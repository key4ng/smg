"""DeepInferEngine gRPC servicer.

Implements ``deepinfer_engine.proto`` in the frontend process, on top of an
:class:`~smg_grpc_servicer.deepinfer.engine_client.EngineClient` of the rank's
engine-core process (or, in tests, anything with the same surface). The
gateway tokenizes, applies the chat template and parses the output; this
servicer only maps a tokenized request to DeepInfer's ``add_request``, streams
the token ids back, and answers the info RPCs. Requests the engine has no
counterpart for (stop strings, penalties, n > 1, ...) are refused by the
gateway before they are built; the proto cannot carry them.
"""

from __future__ import annotations

import asyncio
import hashlib
import logging
import time
from collections.abc import Iterable
from pathlib import Path
from typing import Any, Protocol

import grpc
from smg_grpc_proto import deepinfer_engine_pb2, deepinfer_engine_pb2_grpc

from smg_grpc_servicer.deepinfer.engine_loop import (
    Delta,
    Finished,
    Request,
    Sink,
    TokenLogprob,
)
from smg_grpc_servicer.deepinfer.wire import ModelFacts
from smg_grpc_servicer.tokenizer_bundle import build_tokenizer_zip, tokenizer_chunks

logger = logging.getLogger(__name__)

#: OpenAI caps top logprobs at 20; DeepInfer accepts up to 1000.
MAX_TOP_LOGPROBS = 1000

_SCHEMA_TYPES = {"json_schema": "json", "regex": "regex", "ebnf": "ebnf"}


class Engine(Protocol):
    """What the servicer drives: an ``EngineClient`` (or an ``EngineLoop``)."""

    def submit(self, request: Request, sink: Sink) -> None: ...

    def abort(self, request_ids: Iterable[str]) -> None: ...

    def num_active(self) -> int: ...

    def is_alive(self) -> bool: ...


class RequestError(ValueError):
    """A request the servicer refuses before it reaches the engine."""


def build_request(req: deepinfer_engine_pb2.GenerateRequest, max_model_len: int) -> Request:
    """Map a proto request to engine terms, or raise :class:`RequestError`."""
    if not req.request_id:
        raise RequestError("request_id is required")
    input_ids = list(req.tokenized.input_ids)
    if not input_ids:
        raise RequestError("tokenized.input_ids is empty")
    if len(input_ids) >= max_model_len:
        raise RequestError(
            f"prompt has {len(input_ids)} tokens; the engine's max_model_len is {max_model_len}"
        )

    sp = req.sampling_params
    room = max_model_len - len(input_ids)
    max_tokens = sp.max_tokens if sp.HasField("max_tokens") else room
    params: dict[str, Any] = {
        "temperature": sp.temperature if sp.HasField("temperature") else 1.0,
        "top_p": sp.top_p if sp.top_p > 0 else 1.0,
        "top_k": sp.top_k if sp.top_k > 0 else -1,
        "ignore_eos": sp.ignore_eos,
        # DeepInfer's own default is 16 tokens; an absent max_tokens means
        # "until EOS or the context is full", as for the other engines.
        "max_tokens": max(1, min(max_tokens, room)),
    }
    logprobs: int | None = None
    if sp.HasField("logprobs"):
        logprobs = max(0, min(sp.logprobs, MAX_TOP_LOGPROBS))
        params["top_logprobs"] = logprobs
    if sp.HasField("seed"):
        params["seq_seed"] = sp.seed
    constraint = sp.WhichOneof("constraint")
    if constraint is not None:
        params["so_args"] = {
            "schema_type": _SCHEMA_TYPES[constraint],
            "schema": getattr(sp, constraint),
        }
    return Request(
        request_id=req.request_id,
        prompt_token_ids=input_ids,
        sampling_params=params,
        stop_token_ids=frozenset(sp.stop_token_ids),
        logprobs=logprobs,
    )


def _output_logprobs(entries: list[TokenLogprob] | None, token_ids: list[int]):
    if entries is None:
        return None
    return deepinfer_engine_pb2.OutputLogProbs(
        token_logprobs=[e.logprob for e in entries],
        token_ids=token_ids,
        top_logprobs=[
            deepinfer_engine_pb2.TopLogProbs(values=e.top_logprobs, token_ids=e.top_token_ids)
            for e in entries
        ],
    )


def chunk_response(delta: Delta) -> deepinfer_engine_pb2.GenerateResponse:
    return deepinfer_engine_pb2.GenerateResponse(
        chunk=deepinfer_engine_pb2.GenerateStreamChunk(
            token_ids=delta.token_ids,
            prompt_tokens=delta.prompt_tokens,
            completion_tokens=delta.completion_tokens,
            output_logprobs=_output_logprobs(delta.logprobs, delta.token_ids),
        )
    )


def complete_response(done: Finished) -> deepinfer_engine_pb2.GenerateResponse:
    complete = deepinfer_engine_pb2.GenerateComplete(
        output_ids=done.output_ids,
        finish_reason=done.finish_reason,
        prompt_tokens=done.prompt_tokens,
        completion_tokens=len(done.output_ids),
        output_logprobs=_output_logprobs(done.logprobs, done.output_ids),
    )
    if done.matched_stop_token_id is not None:
        complete.matched_stop_token_id = done.matched_stop_token_id
    return deepinfer_engine_pb2.GenerateResponse(complete=complete)


class DeepInferEngineServicer(deepinfer_engine_pb2_grpc.DeepInferEngineServicer):
    """gRPC front of one DeepInfer engine rank."""

    def __init__(self, engine: Engine, facts: ModelFacts, start_time: float) -> None:
        self._engine = engine
        self._facts = facts
        self._start_time = start_time
        self._shutting_down = False
        self._tokenizer_archive: tuple[bytes, str] | None = None

    def set_shutting_down(self) -> None:
        self._shutting_down = True

    async def Generate(self, request, context):
        try:
            engine_request = build_request(request, self._facts.max_model_len)
        except RequestError as e:
            await context.abort(grpc.StatusCode.INVALID_ARGUMENT, str(e))

        loop = asyncio.get_running_loop()
        events: asyncio.Queue[Delta | Finished] = asyncio.Queue()

        def sink(event: Delta | Finished) -> None:
            loop.call_soon_threadsafe(events.put_nowait, event)

        self._engine.submit(engine_request, sink)
        finished = False
        try:
            while True:
                event = await events.get()
                if isinstance(event, Delta):
                    if request.stream:
                        yield chunk_response(event)
                    continue
                finished = True
                if event.error is not None:
                    await context.abort(grpc.StatusCode.INTERNAL, event.error)
                yield complete_response(event)
                return
        finally:
            # A client that goes away (or a gateway-side abort-on-drop) cancels
            # this generator; stop the request in the engine too.
            if not finished:
                self._engine.abort([engine_request.request_id])

    async def Abort(self, request, context):
        self._engine.abort(request.request_ids)
        return deepinfer_engine_pb2.AbortResponse()

    async def HealthCheck(self, request, context):
        if self._shutting_down:
            return deepinfer_engine_pb2.HealthCheckResponse(
                healthy=False, message="servicer shutting down"
            )
        if not self._engine.is_alive():
            return deepinfer_engine_pb2.HealthCheckResponse(
                healthy=False, message="engine loop is not running"
            )
        return deepinfer_engine_pb2.HealthCheckResponse(healthy=True, message="OK")

    async def GetModelInfo(self, request, context):
        f = self._facts
        return deepinfer_engine_pb2.GetModelInfoResponse(
            model_path=f.model_path,
            is_generation=True,
            max_context_length=f.max_model_len,
            vocab_size=f.vocab_size,
            served_model_name=f.served_model_name,
            model_type=f.model_type,
            architectures=f.architectures,
            eos_token_ids=f.eos_token_ids,
            pad_token_id=f.pad_token_id,
            bos_token_id=f.bos_token_id,
            max_req_input_len=max(0, f.max_model_len - 1),
        )

    async def GetServerInfo(self, request, context):
        f = self._facts
        return deepinfer_engine_pb2.GetServerInfoResponse(
            server_type="deepinfer-grpc",
            active_requests=self._engine.num_active(),
            uptime_seconds=time.time() - self._start_time,
            engine_rank=f.engine_rank,
            expert_parallel_size=f.expert_parallel_size,
            context_parallel_size=f.context_parallel_size,
            max_num_seqs=f.max_num_seqs,
            max_num_batched_tokens=f.max_num_batched_tokens,
        )

    async def GetTokenizer(self, request, context):
        """Stream the model directory's tokenizer files as the shared ZIP
        bundle the gateway's loader expects."""
        try:
            zip_data, sha256 = await self._tokenizer_bundle()
        except Exception as e:
            logger.exception("GetTokenizer failed")
            await context.abort(grpc.StatusCode.INTERNAL, str(e))
        for chunk in tokenizer_chunks(zip_data, sha256):
            yield chunk

    async def _tokenizer_bundle(self) -> tuple[bytes, str]:
        """The tokenizer archive and its SHA-256, built off the event loop on
        the first request and served from memory after that."""
        if self._tokenizer_archive is None:
            self._tokenizer_archive = await asyncio.to_thread(
                _build_tokenizer_archive, self._facts.model_dir
            )
        return self._tokenizer_archive


def _build_tokenizer_archive(model_dir: str) -> tuple[bytes, str]:
    data = build_tokenizer_zip(Path(model_dir)).getvalue()
    return data, hashlib.sha256(data).hexdigest()
