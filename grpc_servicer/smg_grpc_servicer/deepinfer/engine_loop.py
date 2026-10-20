"""The thread that owns a DeepInfer ``LLMEngine`` and drives its step loop.

DeepInfer ships an in-process engine and no server: the caller adds
pre-tokenized requests, calls ``step()`` until nothing is unfinished, and
reads one ``RequestOutput`` per generated token (several per request per step
under speculative decoding). This module turns that into a long-running loop
for a server:

* The engine is built on the loop thread and only ever touched there, so its
  scheduler (C++, not thread-safe) and its CUDA state live on one thread.
  RPC handlers talk to it through :meth:`EngineLoop.submit` and
  :meth:`EngineLoop.abort`, which only enqueue.
* Every iteration calls ``has_unfinished_requests()``. With data parallelism
  that is an all-reduce across the DP ranks, so every rank keeps looping in
  lockstep even when it has no request of its own, which is what DeepInfer's
  expert-parallel collectives inside ``step()`` require.
* Outputs are grouped per request per iteration and handed to the request's
  sink as one :class:`Delta`; the end of a request is one :class:`Finished`.
* The engine stops only on the model's EOS. Stop token ids are enforced here:
  the first one sampled finishes the request with ``stop`` and aborts it in
  the engine. Tokens a speculative step produced after it are dropped.

Nothing here imports gRPC or DeepInfer, so it is tested with a fake engine.
"""

from __future__ import annotations

import logging
import queue
import threading
import time
from collections.abc import Callable, Iterable
from dataclasses import dataclass, field
from typing import Any, Protocol

import numpy as np

logger = logging.getLogger(__name__)

#: How long an idle iteration sleeps. Short: it is also the admission delay
#: of a request arriving at an idle rank.
IDLE_SLEEP_S = 0.002

#: DeepInfer finish reasons that end a request normally. Any other one
#: (``prompt_length_exceeded``, ``insufficient_system_resource``, the
#: xgrammar and vision-encoder failures) is reported as an error.
_NORMAL_FINISH = {"stop", "length", "abort"}


class Engine(Protocol):
    """The part of ``deepinfer.engine.llm_engine.LLMEngine`` the loop uses."""

    def add_request(self, *args: Any, **kwargs: Any) -> Any: ...

    def abort_request(self, request_id: Iterable[str]) -> Any: ...

    def has_unfinished_requests(self) -> bool: ...

    def step(self) -> Iterable[Any]: ...

    def flush_inflight_reqs(self) -> Iterable[Any]: ...


@dataclass
class Request:
    """One generation request, already validated and in engine terms."""

    request_id: str
    prompt_token_ids: list[int]
    #: Keyword arguments for ``deepinfer.sampling_params.SamplingParams``.
    sampling_params: dict[str, Any]
    stop_token_ids: frozenset[int] = frozenset()
    #: Top logprobs per token to report; ``None`` reports no logprobs.
    logprobs: int | None = None


@dataclass
class TokenLogprob:
    logprob: float
    top_token_ids: list[int]
    top_logprobs: list[float]


@dataclass
class Delta:
    """Tokens a request produced in one iteration."""

    token_ids: list[int]
    logprobs: list[TokenLogprob] | None
    prompt_tokens: int
    completion_tokens: int


@dataclass
class Finished:
    """The end of a request. ``error`` is set when it failed in the engine."""

    finish_reason: str
    output_ids: list[int]
    logprobs: list[TokenLogprob] | None
    prompt_tokens: int
    matched_stop_token_id: int | None = None
    error: str | None = None


Sink = Callable[["Delta | Finished"], None]


@dataclass
class _Tracked:
    request: Request
    sink: Sink
    output_ids: list[int] = field(default_factory=list)
    logprobs: list[TokenLogprob] | None = None
    prompt_tokens: int = 0
    pending_ids: list[int] = field(default_factory=list)
    pending_logprobs: list[TokenLogprob] = field(default_factory=list)


class EngineLoop:
    """Owns an engine and runs its step loop; see the module docstring.

    :meth:`run_forever` runs the loop on the calling thread (the engine-core
    process's main thread); :meth:`start` runs it on a new thread.

    ``engine_factory`` builds the engine on the loop thread.
    ``clone_output`` turns one entry of ``step()``'s result into an object
    with DeepInfer ``RequestOutput``'s attributes (in production,
    ``RequestOutput.clone``: the entries are pointers valid only until the
    next step).
    """

    def __init__(
        self,
        engine_factory: Callable[[], Engine],
        clone_output: Callable[[Any], Any],
        *,
        user_id: str = "smg",
        idle_sleep_s: float = IDLE_SLEEP_S,
    ) -> None:
        self._engine_factory = engine_factory
        self._clone_output = clone_output
        self._user_id = user_id
        self._idle_sleep_s = idle_sleep_s
        self._inbox: queue.SimpleQueue[tuple[str, Any]] = queue.SimpleQueue()
        self._tracked: dict[str, _Tracked] = {}
        self._stop = threading.Event()
        self._ready = threading.Event()
        self._thread: threading.Thread | None = None
        self._running = False
        self.engine: Engine | None = None
        self.startup_error: BaseException | None = None
        self.loop_error: BaseException | None = None

    # ── Control, from any thread ────────────────────────────────────────

    def start(self) -> None:
        self._thread = threading.Thread(
            target=self.run_forever, name="deepinfer-engine", daemon=True
        )
        self._thread.start()

    def wait_ready(self, timeout: float | None = None) -> bool:
        """Block until the engine is built. False if building it failed."""
        self._ready.wait(timeout)
        return self._ready.is_set() and self.startup_error is None

    def stop(self, timeout: float | None = 10.0) -> None:
        self._stop.set()
        if self._thread is not None:
            self._thread.join(timeout)

    def is_alive(self) -> bool:
        return self._running and self.loop_error is None

    def submit(self, request: Request, sink: Sink) -> None:
        """Queue a request; its outputs go to ``sink`` on the loop thread."""
        self._inbox.put(("add", (request, sink)))

    def abort(self, request_ids: Iterable[str]) -> None:
        """Queue an abort; each still-running request finishes with ``abort``."""
        self._inbox.put(("abort", list(request_ids)))

    def num_active(self) -> int:
        return len(self._tracked)

    # ── Loop thread ─────────────────────────────────────────────────────

    def run_forever(self) -> None:
        """Build the engine, then step until :meth:`stop`. A failure to build
        is reported through :meth:`wait_ready`; a failure of the loop fails
        every running request and ends it (``loop_error``)."""
        self._running = True
        try:
            try:
                self.engine = self._engine_factory()
            except BaseException as e:  # noqa: BLE001 - reported through wait_ready
                logger.exception("Failed to build the DeepInfer engine")
                self.startup_error = e
                return
            finally:
                self._ready.set()
            try:
                while not self._stop.is_set():
                    self.run_once()
            except BaseException as e:  # noqa: BLE001 - the loop must report, not vanish
                logger.exception("DeepInfer engine loop failed")
                self.loop_error = e
                self._fail_all(f"engine loop failed: {e}")
        finally:
            self._running = False

    def run_once(self) -> None:
        """One iteration: admit and abort, step, deliver. Public for tests."""
        engine = self.engine
        assert engine is not None
        self._drain_inbox(engine)
        unfinished = engine.has_unfinished_requests()
        raw = engine.step() if unfinished else engine.flush_inflight_reqs()
        outputs = [self._clone_output(ptr) for ptr in raw]
        for output in outputs:
            self._on_output(engine, output)
        self._flush_deltas()
        if not unfinished and not outputs:
            time.sleep(self._idle_sleep_s)

    def _drain_inbox(self, engine: Engine) -> None:
        while True:
            try:
                kind, payload = self._inbox.get_nowait()
            except queue.Empty:
                return
            if kind == "add":
                self._admit(engine, *payload)
            else:
                self._abort(engine, payload)

    def _admit(self, engine: Engine, request: Request, sink: Sink) -> None:
        if request.request_id in self._tracked:
            sink(_error(request, "duplicate request_id"))
            return
        # A vision model's engine routes every request through its multimodal
        # processor, which is what marks it schedulable; DeepInfer's own `LLM`
        # wrapper hands a text request an empty image list for that reason.
        multi_modal_data = None
        if getattr(engine, "multi_modal_processor", None) is not None:
            multi_modal_data = {"image_bytes": [], "image_sizes": []}
        try:
            seq = engine.add_request(
                user_id=self._user_id,
                request_id=request.request_id,
                sampling_params=dict(request.sampling_params),
                prompt_token_ids=np.asarray(request.prompt_token_ids, dtype=np.int32),
                multi_modal_data=multi_modal_data,
            )
        except Exception as e:  # noqa: BLE001 - one bad request must not stop the loop
            logger.warning("Rejected request %s: %s", request.request_id, e)
            sink(_error(request, str(e)))
            return
        if seq is None:
            # add_request drops a prompt with out-of-vocabulary ids and only logs.
            sink(_error(request, "prompt contains token ids outside the vocabulary"))
            return
        self._tracked[request.request_id] = _Tracked(
            request=request,
            sink=sink,
            logprobs=[] if request.logprobs is not None else None,
            prompt_tokens=len(request.prompt_token_ids),
        )

    def _abort(self, engine: Engine, request_ids: list[str]) -> None:
        live = [rid for rid in request_ids if rid in self._tracked]
        if not live:
            return
        engine.abort_request(live)
        for rid in live:
            self._finish(rid, "abort")

    def _on_output(self, engine: Engine, output: Any) -> None:
        tracked = self._tracked.get(output.request_id)
        if tracked is None:
            # Finished here already (stop token, abort); a speculative step
            # or the engine's own abort output can still name it.
            return
        if output.prompt_len:
            tracked.prompt_tokens = output.prompt_len
        reason = output.finish_reason if output.finished else ""
        carries_token = output.token_id >= 0 and reason in ("", "stop", "length")
        if carries_token:
            token_id = int(output.token_id)
            tracked.output_ids.append(token_id)
            tracked.pending_ids.append(token_id)
            if tracked.logprobs is not None:
                entry = _logprob_of(output, tracked.request.logprobs or 0)
                tracked.logprobs.append(entry)
                tracked.pending_logprobs.append(entry)
            if token_id in tracked.request.stop_token_ids:
                engine.abort_request([output.request_id])
                self._finish(output.request_id, "stop", matched=token_id)
                return
        if output.finished:
            if reason in _NORMAL_FINISH:
                matched = None
                if reason == "stop" and tracked.output_ids:
                    matched = tracked.output_ids[-1]
                self._finish(output.request_id, reason, matched=matched)
            else:
                self._finish(
                    output.request_id,
                    reason,
                    error=f"DeepInfer finished the request with {reason!r}",
                )

    def _flush_deltas(self) -> None:
        for tracked in self._tracked.values():
            if tracked.pending_ids:
                self._send_delta(tracked)

    def _send_delta(self, tracked: _Tracked) -> None:
        delta = Delta(
            token_ids=tracked.pending_ids,
            logprobs=tracked.pending_logprobs if tracked.logprobs is not None else None,
            prompt_tokens=tracked.prompt_tokens,
            completion_tokens=len(tracked.output_ids),
        )
        tracked.pending_ids = []
        tracked.pending_logprobs = []
        tracked.sink(delta)

    def _finish(
        self,
        request_id: str,
        reason: str,
        *,
        matched: int | None = None,
        error: str | None = None,
    ) -> None:
        tracked = self._tracked.pop(request_id)
        if tracked.pending_ids:
            self._send_delta(tracked)
        tracked.sink(
            Finished(
                finish_reason=reason,
                output_ids=tracked.output_ids,
                logprobs=tracked.logprobs,
                prompt_tokens=tracked.prompt_tokens,
                matched_stop_token_id=matched,
                error=error,
            )
        )

    def _fail_all(self, message: str) -> None:
        for request_id in list(self._tracked):
            self._finish(request_id, "abort", error=message)


def _error(request: Request, message: str) -> Finished:
    return Finished(
        finish_reason="abort",
        output_ids=[],
        logprobs=None,
        prompt_tokens=len(request.prompt_token_ids),
        error=message,
    )


def _logprob_of(output: Any, top_n: int) -> TokenLogprob:
    top_ids = list(output.top_token_ids)[:top_n] if top_n else []
    top_lps = list(output.top_logprobs)[:top_n] if top_n else []
    return TokenLogprob(
        logprob=float(output.token_logprobs),
        top_token_ids=[int(t) for t in top_ids],
        top_logprobs=[float(v) for v in top_lps],
    )
