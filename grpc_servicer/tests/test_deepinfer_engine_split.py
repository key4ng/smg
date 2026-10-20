"""The engine-core / frontend split: an EngineCore on a thread with a fake
engine, an EngineClient on the test's event loop, real ZMQ IPC between them."""

from __future__ import annotations

import asyncio
import threading

import pytest
from smg_grpc_servicer.deepinfer import wire
from smg_grpc_servicer.deepinfer.engine_client import EngineClient, EngineStartupError
from smg_grpc_servicer.deepinfer.engine_core import EngineCore
from smg_grpc_servicer.deepinfer.engine_loop import Delta, EngineLoop, Finished, Request
from test_deepinfer_engine_loop import FakeEngine, tok

FACTS = wire.ModelFacts(
    model_path="m", model_dir="/m", served_model_name="m", max_model_len=100, vocab_size=10
)


class Harness:
    def __init__(self, tmp_path, engine_factory):
        self.addresses = wire.rank_addresses(0, str(tmp_path))
        self.loop = EngineLoop(engine_factory, lambda o: o, idle_sleep_s=0.001)
        self.core = EngineCore(self.loop, self.addresses, lambda _engine: FACTS)
        self.exit_code: int | None = None
        self.thread = threading.Thread(target=self._run, daemon=True)
        self.thread.start()

    def _run(self):
        self.exit_code = self.core.run()

    def stop(self):
        self.core.stop()
        self.thread.join(timeout=5)


async def run_request(client: EngineClient, request: Request) -> list:
    events: asyncio.Queue = asyncio.Queue()
    client.submit(request, events.put_nowait)
    out = []
    while True:
        event = await asyncio.wait_for(events.get(), timeout=5)
        out.append(event)
        if isinstance(event, Finished):
            return out


def test_requests_stream_through_the_engine_core(tmp_path):
    engine = FakeEngine(
        {
            "a": [[tok("a", 10)], [tok("a", 11, finished=True, finish_reason="length")]],
            "b": [[tok("b", 20), tok("b", 7)], [tok("b", 21)]],
        }
    )
    harness = Harness(tmp_path, lambda: engine)

    async def scenario():
        client = EngineClient(harness.addresses)
        facts = await asyncio.wait_for(client.start(), timeout=5)
        assert facts == FACTS
        assert client.is_alive()
        a, b = await asyncio.gather(
            run_request(client, Request("a", [1, 2], {})),
            run_request(client, Request("b", [1], {}, stop_token_ids=frozenset({7}))),
        )
        assert a[-1].finish_reason == "length" and a[-1].output_ids == [10, 11]
        assert [e.token_ids for e in a if isinstance(e, Delta)] == [[10], [11]]
        assert b[-1].finish_reason == "stop" and b[-1].matched_stop_token_id == 7
        assert client.num_active() == 0
        await client.close()

    try:
        asyncio.run(scenario())
    finally:
        harness.stop()
    assert harness.exit_code == 0
    assert engine.aborted == ["b"]


def test_abort_and_duplicate_ids(tmp_path):
    engine = FakeEngine({"r": [[tok("r", i)] for i in range(1000)]})
    harness = Harness(tmp_path, lambda: engine)

    async def scenario():
        client = EngineClient(harness.addresses)
        await asyncio.wait_for(client.start(), timeout=5)
        events: asyncio.Queue = asyncio.Queue()
        client.submit(Request("r", [1], {}), events.put_nowait)
        first = await asyncio.wait_for(events.get(), timeout=5)
        assert isinstance(first, Delta)

        dup: list = []
        client.submit(Request("r", [1], {}), dup.append)
        assert dup[-1].error == "duplicate request_id"

        client.abort(["r"])
        while True:
            event = await asyncio.wait_for(events.get(), timeout=5)
            if isinstance(event, Finished):
                break
        assert event.finish_reason == "abort"
        await client.close()

    try:
        asyncio.run(scenario())
    finally:
        harness.stop()
    assert engine.aborted == ["r"]


def test_engine_build_failure_reaches_the_frontend(tmp_path):
    def boom():
        raise RuntimeError("no GPU")

    harness = Harness(tmp_path, boom)

    async def scenario():
        client = EngineClient(harness.addresses)
        with pytest.raises(EngineStartupError, match="no GPU"):
            await asyncio.wait_for(client.start(), timeout=5)
        await client.close()

    asyncio.run(scenario())
    harness.thread.join(timeout=5)
    assert harness.exit_code == 1


def test_loop_failure_fails_running_requests(tmp_path):
    engine = FakeEngine({"r": [[tok("r", 10)]] + [[tok("r", 11)]] * 1000})
    calls = {"n": 0}
    real_step = engine.step

    def flaky_step():
        calls["n"] += 1
        if calls["n"] == 3:
            raise RuntimeError("CUDA error")
        return real_step()

    engine.step = flaky_step
    harness = Harness(tmp_path, lambda: engine)

    async def scenario():
        client = EngineClient(harness.addresses)
        await asyncio.wait_for(client.start(), timeout=5)
        events = await run_request(client, Request("r", [1], {}))
        assert "CUDA error" in events[-1].error
        for _ in range(50):
            if not client.is_alive():
                break
            await asyncio.sleep(0.05)
        assert not client.is_alive()
        late: list = []
        client.submit(Request("late", [1], {}), late.append)
        assert "engine core is down" in late[-1].error
        await client.close()

    asyncio.run(scenario())
    harness.thread.join(timeout=5)
    assert harness.exit_code == 1


def test_frontend_notices_a_core_that_never_starts(tmp_path):
    addresses = wire.rank_addresses(0, str(tmp_path))

    async def scenario():
        # A pid that is not running: the startup wait gives up instead of hanging.
        client = EngineClient(addresses, engine_pid=2**22 + 12345)
        with pytest.raises(EngineStartupError, match="exited"):
            await asyncio.wait_for(client.start(), timeout=5)
        await client.close()

    asyncio.run(scenario())
