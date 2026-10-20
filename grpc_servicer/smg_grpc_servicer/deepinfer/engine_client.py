"""The gRPC frontend's client of one rank's engine-core process.

It offers the frontend what :class:`EngineLoop` offers in-process
(``submit``, ``abort``, ``num_active``, ``is_alive``), over the ZMQ IPC
sockets of :mod:`smg_grpc_servicer.deepinfer.engine_core`. A reader task on
the frontend's event loop routes each event to its request's sink. The
frontend imports neither torch nor DeepInfer.
"""

from __future__ import annotations

import asyncio
import logging
import os
from collections.abc import Iterable

import zmq
import zmq.asyncio

from smg_grpc_servicer.deepinfer import wire
from smg_grpc_servicer.deepinfer.engine_loop import Finished, Request, Sink

logger = logging.getLogger(__name__)

#: How often the startup wait checks that the engine-core process is alive.
_STARTUP_POLL_S = 1.0


class EngineStartupError(RuntimeError):
    """The engine core failed to build its engine, or exited before it did."""


class EngineClient:
    """Async client of an engine core; see the module docstring.

    ``engine_pid`` is the engine-core process, watched for liveness (the
    launcher owns it; a dead core also takes the frontend down there).
    """

    def __init__(
        self,
        addresses: wire.RankAddresses,
        engine_pid: int | None = None,
        context: zmq.asyncio.Context | None = None,
    ) -> None:
        self._addresses = addresses
        self._engine_pid = engine_pid
        self._ctx = context or zmq.asyncio.Context.instance()
        self._input: zmq.asyncio.Socket | None = None
        self._output: zmq.asyncio.Socket | None = None
        self._sinks: dict[str, Sink] = {}
        self._reader: asyncio.Task | None = None
        self._ready = False
        self._dead: str | None = None

    async def start(self) -> wire.ModelFacts:
        """Connect and wait for the engine to be built; returns its facts."""
        self._input = self._ctx.socket(zmq.PUSH)
        # Never block a submit: the engine core drains its input every step.
        self._input.setsockopt(zmq.SNDHWM, 0)
        self._input.setsockopt(zmq.LINGER, 0)
        self._input.connect(self._addresses.input)
        self._output = self._ctx.socket(zmq.PULL)
        self._output.setsockopt(zmq.RCVHWM, 0)
        self._output.connect(self._addresses.output)

        while True:
            if await self._output.poll(int(_STARTUP_POLL_S * 1000)):
                break
            if not self._engine_process_alive():
                raise EngineStartupError("the engine-core process exited before it was ready")
        kind, payload = wire.decode(await self._output.recv())
        if kind == wire.FAILED:
            raise EngineStartupError(f"the engine core failed to start: {payload}")
        if kind != wire.READY:
            raise EngineStartupError(f"unexpected first message from the engine core: {kind!r}")
        self._ready = True
        self._reader = asyncio.create_task(self._read_events(), name="deepinfer-events")
        return payload

    async def close(self) -> None:
        if self._reader is not None:
            self._reader.cancel()
            try:
                await self._reader
            except asyncio.CancelledError:
                pass
        for sock in (self._input, self._output):
            if sock is not None:
                sock.close(linger=0)

    # ── The EngineLoop surface ───────────────────────────────────────────

    def submit(self, request: Request, sink: Sink) -> None:
        if self._dead is not None:
            sink(_failure(request, f"engine core is down: {self._dead}"))
            return
        if request.request_id in self._sinks:
            sink(_failure(request, "duplicate request_id"))
            return
        self._sinks[request.request_id] = sink
        self._send((wire.ADD, request))

    def abort(self, request_ids: Iterable[str]) -> None:
        live = [rid for rid in request_ids if rid in self._sinks]
        if live and self._dead is None:
            self._send((wire.ABORT, live))

    def num_active(self) -> int:
        return len(self._sinks)

    def is_alive(self) -> bool:
        return self._ready and self._dead is None and self._engine_process_alive()

    # ── Internals ────────────────────────────────────────────────────────

    def _send(self, message: tuple) -> None:
        assert self._input is not None, "start() first"
        sent = self._input.send(wire.encode(message))
        sent.add_done_callback(_log_send_failure)

    async def _read_events(self) -> None:
        assert self._output is not None
        while True:
            kind, payload = wire.decode(await self._output.recv())
            if kind == wire.EVENTS:
                for request_id, event in payload:
                    sink = self._sinks.get(request_id)
                    if sink is None:
                        continue
                    if isinstance(event, Finished):
                        del self._sinks[request_id]
                    sink(event)
            elif kind == wire.DEAD:
                logger.error("DeepInfer engine core failed: %s", payload)
                self._mark_dead(str(payload))
                return
            else:
                logger.warning("Ignoring unknown message %r from the engine core", kind)

    def _mark_dead(self, reason: str) -> None:
        self._dead = reason
        sinks, self._sinks = self._sinks, {}
        for sink in sinks.values():
            sink(
                Finished(
                    finish_reason="abort",
                    output_ids=[],
                    logprobs=None,
                    prompt_tokens=0,
                    error=f"engine core is down: {reason}",
                )
            )

    def _engine_process_alive(self) -> bool:
        if self._engine_pid is None:
            return True
        try:
            os.kill(self._engine_pid, 0)
        except ProcessLookupError:
            return False
        except PermissionError:
            return True
        return True


def _failure(request: Request, message: str) -> Finished:
    return Finished(
        finish_reason="abort",
        output_ids=[],
        logprobs=None,
        prompt_tokens=len(request.prompt_token_ids),
        error=message,
    )


def _log_send_failure(future: asyncio.Future) -> None:
    if not future.cancelled() and future.exception() is not None:
        logger.error("Failed to send to the engine core: %s", future.exception())
