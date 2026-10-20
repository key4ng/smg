"""The engine-core process of one DeepInfer DP rank.

It owns the GPU: it builds DeepInfer's ``LLMEngine`` and runs the
:class:`EngineLoop` on its main thread, with nothing else competing for the
interpreter but two I/O threads, as vLLM's ``EngineCoreProc`` does:

* the input thread receives ``add`` / ``abort`` messages from the frontend and
  queues them for the loop;
* the output thread sends the ``ready`` (or ``failed``) message once the engine
  is built, then the loop's per-request events, batched per send.

The gRPC frontend (:mod:`smg_grpc_servicer.deepinfer.engine_client`) runs in
another process, so request decoding and protobuf encoding never hold up a
step. See :mod:`smg_grpc_servicer.deepinfer.wire` for the messages.
"""

from __future__ import annotations

import logging
import queue
import threading
from collections.abc import Callable
from typing import Any

import zmq

from smg_grpc_servicer.deepinfer import wire
from smg_grpc_servicer.deepinfer.engine_loop import Delta, EngineLoop, Finished, Request

logger = logging.getLogger(__name__)

#: How often the I/O threads look up from a blocking wait to check for shutdown.
_POLL_MS = 100
#: How long the output socket keeps unsent messages (a final ``dead``) at close.
_LINGER_MS = 2000


class EngineCore:
    """Wires an :class:`EngineLoop` to the frontend's sockets.

    ``facts_of`` reads the :class:`~wire.ModelFacts` off the built engine.
    :meth:`run` blocks on the calling thread until the loop stops.
    """

    def __init__(
        self,
        engine_loop: EngineLoop,
        addresses: wire.RankAddresses,
        facts_of: Callable[[Any], wire.ModelFacts],
        context: zmq.Context | None = None,
    ) -> None:
        self._loop = engine_loop
        self._addresses = addresses
        self._facts_of = facts_of
        self._ctx = context or zmq.Context.instance()
        self._outbox: queue.SimpleQueue[tuple[str, Delta | Finished] | None] = queue.SimpleQueue()
        self._closing = threading.Event()

    def stop(self) -> None:
        """Stop the step loop; :meth:`run` then returns."""
        self._loop.stop(timeout=0)

    def run(self) -> int:
        """Serve until the loop stops. Returns the process exit code."""
        input_sock = self._ctx.socket(zmq.PULL)
        input_sock.bind(self._addresses.input)
        output_sock = self._ctx.socket(zmq.PUSH)
        output_sock.setsockopt(zmq.LINGER, _LINGER_MS)
        output_sock.bind(self._addresses.output)

        reader = threading.Thread(
            target=self._read_inputs, args=(input_sock,), name="deepinfer-input", daemon=True
        )
        writer = threading.Thread(
            target=self._write_outputs, args=(output_sock,), name="deepinfer-output", daemon=True
        )
        reader.start()
        writer.start()
        try:
            self._loop.run_forever()
        finally:
            self._closing.set()
            self._outbox.put(None)
            writer.join()
            reader.join()
            input_sock.close(linger=0)
            output_sock.close()
        if self._loop.startup_error is not None or self._loop.loop_error is not None:
            return 1
        return 0

    # ── I/O threads ──────────────────────────────────────────────────────

    def _read_inputs(self, sock: zmq.Socket) -> None:
        poller = zmq.Poller()
        poller.register(sock, zmq.POLLIN)
        while not self._closing.is_set():
            if not poller.poll(_POLL_MS):
                continue
            kind, payload = wire.decode(sock.recv())
            if kind == wire.ADD:
                self._loop.submit(payload, self._sink_for(payload))
            elif kind == wire.ABORT:
                self._loop.abort(payload)
            else:
                logger.warning("Ignoring unknown message %r from the frontend", kind)

    def _sink_for(self, request: Request) -> Callable[[Delta | Finished], None]:
        request_id = request.request_id
        outbox = self._outbox

        def sink(event: Delta | Finished) -> None:
            outbox.put((request_id, event))

        return sink

    def _write_outputs(self, sock: zmq.Socket) -> None:
        self._loop.wait_ready()
        if self._loop.startup_error is not None:
            sock.send(wire.encode((wire.FAILED, str(self._loop.startup_error))))
            return
        sock.send(wire.encode((wire.READY, self._facts_of(self._loop.engine))))
        while True:
            first = self._outbox.get()
            batch = [] if first is None else [first]
            done = first is None
            # Everything already queued goes out in the same message: one
            # loop iteration's events for every request.
            while True:
                try:
                    item = self._outbox.get_nowait()
                except queue.Empty:
                    break
                if item is None:
                    done = True
                else:
                    batch.append(item)
            if batch:
                sock.send(wire.encode((wire.EVENTS, batch)))
            if done:
                if self._loop.loop_error is not None:
                    sock.send(wire.encode((wire.DEAD, str(self._loop.loop_error))))
                return
