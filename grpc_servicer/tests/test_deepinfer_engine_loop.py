"""The DeepInfer engine loop and request mapping, against a fake engine."""

from __future__ import annotations

from dataclasses import dataclass, field

import pytest
from smg_grpc_servicer.deepinfer.engine_loop import Delta, EngineLoop, Finished, Request


@dataclass
class FakeOutput:
    request_id: str
    token_id: int
    finished: bool = False
    finish_reason: str = ""
    prompt_len: int = 0
    token_logprobs: float = -0.5
    top_logprobs: list[float] = field(default_factory=lambda: [-0.5, -1.0, -2.0])
    top_token_ids: list[int] = field(default_factory=lambda: [1, 2, 3])


class FakeEngine:
    """Hands out scripted outputs per step, like DeepInfer's ``step()``: each
    entry is one generated token; several per request under speculation."""

    def __init__(self, script: dict[str, list[list[FakeOutput]]] | None = None):
        self.script = script or {}
        self.added: list[dict] = []
        self.aborted: list[str] = []
        self.reject_with_none: set[str] = set()
        self.multi_modal_processor = None

    def add_request(
        self, *, user_id, request_id, sampling_params, prompt_token_ids, multi_modal_data
    ):
        self.added.append(
            {
                "request_id": request_id,
                "sampling_params": sampling_params,
                "multi_modal_data": multi_modal_data,
            }
        )
        if request_id in self.reject_with_none:
            return None
        return object()

    def abort_request(self, request_ids):
        self.aborted.extend(request_ids)
        for rid in request_ids:
            self.script.pop(rid, None)

    def _live(self):
        added = {a["request_id"] for a in self.added}
        return [steps for rid, steps in self.script.items() if rid in added and steps]

    def has_unfinished_requests(self):
        return bool(self._live())

    def step(self):
        # Like the engine, only requests it was given produce outputs.
        out = []
        for steps in self._live():
            out.extend(steps.pop(0))
        return out

    def flush_inflight_reqs(self):
        return []


def make_loop(engine: FakeEngine) -> EngineLoop:
    loop = EngineLoop(lambda: engine, lambda o: o, idle_sleep_s=0)
    loop.engine = engine
    return loop


def collect(loop: EngineLoop, request: Request, steps: int = 10) -> list:
    events: list = []
    loop.submit(request, events.append)
    for _ in range(steps):
        loop.run_once()
        if events and isinstance(events[-1], Finished):
            break
    return events


def tok(rid, token, **kw):
    return FakeOutput(request_id=rid, token_id=token, prompt_len=3, **kw)


def test_streams_one_delta_per_step_and_finishes_on_eos():
    engine = FakeEngine(
        {
            "r": [
                [tok("r", 10)],
                [tok("r", 11), tok("r", 12)],
                [tok("r", 2, finished=True, finish_reason="stop")],
            ]
        }
    )
    events = collect(make_loop(engine), Request("r", [1, 2, 3], {"max_tokens": 8}))

    deltas = [e for e in events if isinstance(e, Delta)]
    assert [d.token_ids for d in deltas] == [[10], [11, 12], [2]]
    assert deltas[-1].completion_tokens == 4
    done = events[-1]
    assert isinstance(done, Finished) and done.error is None
    assert done.finish_reason == "stop"
    assert done.output_ids == [10, 11, 12, 2]
    assert done.matched_stop_token_id == 2
    assert done.logprobs is None


def test_stop_token_id_finishes_and_aborts_and_drops_later_speculative_tokens():
    engine = FakeEngine({"r": [[tok("r", 10), tok("r", 7), tok("r", 11)], [tok("r", 12)]]})
    loop = make_loop(engine)
    events = collect(loop, Request("r", [1, 2, 3], {}, stop_token_ids=frozenset({7})))

    done = events[-1]
    assert done.finish_reason == "stop"
    assert done.output_ids == [10, 7]
    assert done.matched_stop_token_id == 7
    assert engine.aborted == ["r"]
    assert loop.num_active() == 0


def test_length_finish_and_logprobs():
    engine = FakeEngine(
        {"r": [[tok("r", 10)], [tok("r", 11, finished=True, finish_reason="length")]]}
    )
    events = collect(make_loop(engine), Request("r", [1, 2, 3], {}, logprobs=2))

    done = events[-1]
    assert done.finish_reason == "length"
    assert done.matched_stop_token_id is None
    assert [lp.logprob for lp in done.logprobs] == [-0.5, -0.5]
    assert done.logprobs[0].top_token_ids == [1, 2]
    assert done.logprobs[0].top_logprobs == [-0.5, -1.0]


def test_abort_finishes_the_request_and_ignores_later_outputs():
    engine = FakeEngine({"r": [[tok("r", 10)], [tok("r", 11)], [tok("r", 12)]]})
    loop = make_loop(engine)
    events: list = []
    loop.submit(Request("r", [1, 2, 3], {}), events.append)
    loop.run_once()
    loop.abort(["r", "unknown"])
    loop.run_once()

    assert isinstance(events[-1], Finished)
    assert events[-1].finish_reason == "abort"
    assert events[-1].output_ids == [10]
    assert engine.aborted == ["r"]


@pytest.mark.parametrize("reason", ["prompt_length_exceeded", "insufficient_system_resource"])
def test_engine_side_failures_are_errors(reason):
    engine = FakeEngine({"r": [[tok("r", -1, finished=True, finish_reason=reason)]]})
    done = collect(make_loop(engine), Request("r", [1, 2, 3], {}))[-1]
    assert done.error is not None and reason in done.error
    assert done.output_ids == []


def test_rejected_prompt_and_duplicate_ids_are_errors():
    engine = FakeEngine({"dup": [[tok("dup", 10)], [tok("dup", 11)]]})
    engine.reject_with_none.add("bad")
    loop = make_loop(engine)

    bad = collect(loop, Request("bad", [99999], {}))
    assert "outside the vocabulary" in bad[-1].error

    first: list = []
    second: list = []
    loop.submit(Request("dup", [1], {}), first.append)
    loop.submit(Request("dup", [1], {}), second.append)
    loop.run_once()
    assert second[-1].error == "duplicate request_id"
    assert loop.num_active() == 1


def test_sampling_params_reach_the_engine_unchanged():
    engine = FakeEngine({"r": [[tok("r", 2, finished=True, finish_reason="stop")]]})
    params = {"temperature": 0.3, "top_k": -1, "max_tokens": 5, "seq_seed": 7}
    collect(make_loop(engine), Request("r", [1, 2, 3], params))
    assert engine.added[0]["sampling_params"] == params


def test_vision_engine_gets_an_empty_image_list():
    engine = FakeEngine({"r": [[tok("r", 2, finished=True, finish_reason="stop")]]})
    collect(make_loop(engine), Request("r", [1, 2, 3], {}))
    assert engine.added[0]["multi_modal_data"] is None

    engine = FakeEngine({"r": [[tok("r", 2, finished=True, finish_reason="stop")]]})
    engine.multi_modal_processor = object()
    collect(make_loop(engine), Request("r", [1, 2, 3], {}))
    assert engine.added[0]["multi_modal_data"] == {"image_bytes": [], "image_sizes": []}


def test_factory_failure_is_reported_by_wait_ready():
    def boom():
        raise RuntimeError("no GPU")

    loop = EngineLoop(boom, lambda o: o)
    loop.start()
    assert loop.wait_ready(timeout=5) is False
    assert isinstance(loop.startup_error, RuntimeError)
    assert not loop.is_alive()
