"""The DeepInfer servicer's proto mapping and launch layout."""

from __future__ import annotations

import sys

import pytest
from smg_grpc_proto import deepinfer_engine_pb2 as pb
from smg_grpc_servicer.deepinfer.engine_loop import Finished, TokenLogprob
from smg_grpc_servicer.deepinfer.server import local_ranks, visible_devices
from smg_grpc_servicer.deepinfer.servicer import (
    RequestError,
    build_request,
    complete_response,
)


def request(**sampling) -> pb.GenerateRequest:
    return pb.GenerateRequest(
        request_id="r",
        tokenized=pb.TokenizedInput(input_ids=[1, 2, 3]),
        sampling_params=pb.SamplingParams(**sampling),
    )


def test_defaults_fill_the_context_and_disable_top_k():
    req = build_request(request(), max_model_len=100)
    assert req.sampling_params == {
        "temperature": 1.0,
        "top_p": 1.0,
        "top_k": -1,
        "ignore_eos": False,
        "max_tokens": 97,
    }
    assert req.logprobs is None
    assert req.stop_token_ids == frozenset()


def test_explicit_fields_map_to_deepinfer_names():
    req = build_request(
        request(
            temperature=0.0,
            top_p=0.9,
            top_k=40,
            max_tokens=500,
            stop_token_ids=[5, 6],
            logprobs=3,
            seed=11,
            json_schema='{"type":"object"}',
        ),
        max_model_len=100,
    )
    sp = req.sampling_params
    assert sp["temperature"] == 0.0
    assert sp["top_p"] == pytest.approx(0.9)
    assert sp["top_k"] == 40
    assert sp["max_tokens"] == 97  # clamped to the room left in the context
    assert sp["top_logprobs"] == 3
    assert sp["seq_seed"] == 11
    assert sp["so_args"] == {"schema_type": "json", "schema": '{"type":"object"}'}
    assert req.stop_token_ids == frozenset({5, 6})
    assert req.logprobs == 3


@pytest.mark.parametrize(("field", "schema_type"), [("regex", "regex"), ("ebnf", "ebnf")])
def test_constraints(field, schema_type):
    req = build_request(request(**{field: "x"}), max_model_len=100)
    assert req.sampling_params["so_args"] == {"schema_type": schema_type, "schema": "x"}


def test_refusals():
    with pytest.raises(RequestError, match="max_model_len"):
        build_request(request(), max_model_len=3)
    with pytest.raises(RequestError, match="empty"):
        build_request(pb.GenerateRequest(request_id="r"), max_model_len=100)
    with pytest.raises(RequestError, match="request_id"):
        build_request(
            pb.GenerateRequest(tokenized=pb.TokenizedInput(input_ids=[1])), max_model_len=100
        )


def test_complete_response_carries_ids_logprobs_and_matched_stop():
    done = Finished(
        finish_reason="stop",
        output_ids=[10, 2],
        logprobs=[TokenLogprob(-0.1, [10], [-0.1]), TokenLogprob(-0.2, [2], [-0.2])],
        prompt_tokens=3,
        matched_stop_token_id=2,
    )
    c = complete_response(done).complete
    assert list(c.output_ids) == [10, 2]
    assert c.completion_tokens == 2
    assert c.matched_stop_token_id == 2
    assert list(c.output_logprobs.token_ids) == [10, 2]
    assert list(c.output_logprobs.top_logprobs[1].token_ids) == [2]

    no_match = complete_response(
        Finished(finish_reason="length", output_ids=[], logprobs=None, prompt_tokens=3)
    ).complete
    assert not no_match.HasField("matched_stop_token_id")
    assert not no_match.HasField("output_logprobs")


def test_rank_layout_matches_deepinfer_launcher():
    assert local_ranks(8, node_rank=1, nnodes=2) == [4, 5, 6, 7]
    assert visible_devices(5, cp_size=1, device_count=4, offset=0) == "1"
    assert visible_devices(1, cp_size=2, device_count=4, offset=0) == "2,3"
    assert visible_devices(0, cp_size=1, device_count=4, offset=4) == "4"


def _stubborn(pid_file: str) -> None:
    import signal
    import subprocess
    import time

    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    child = subprocess.Popen(["sleep", "60"])
    with open(pid_file, "w") as f:
        f.write(str(child.pid))
    time.sleep(60)


def _crash() -> None:
    import time

    time.sleep(1.0)
    raise SystemExit(3)


@pytest.mark.skipif(not sys.platform.startswith("linux"), reason="uses /proc and prctl")
def test_supervisor_kills_stuck_processes_and_their_children(tmp_path, monkeypatch):
    import multiprocessing
    import os
    import signal
    import time

    from smg_grpc_servicer.deepinfer import server

    monkeypatch.setattr(server, "STOP_GRACE_S", 1.0)
    pid_file = tmp_path / "grandchild"
    ctx = multiprocessing.get_context("spawn")
    procs = {
        "stubborn": ctx.Process(target=_stubborn, args=(str(pid_file),)),
        "crash": ctx.Process(target=_crash),
    }
    for p in procs.values():
        p.start()
    old = {s: signal.getsignal(s) for s in (signal.SIGTERM, signal.SIGINT)}
    try:
        start = time.monotonic()
        assert server._supervise(procs) == 1
        assert time.monotonic() - start < 20
    finally:
        for s, h in old.items():
            signal.signal(s, h)
    grandchild = int(pid_file.read_text())
    time.sleep(0.2)
    assert grandchild not in server._descendants(os.getpid())
    with pytest.raises(ProcessLookupError):
        os.kill(grandchild, 0)
