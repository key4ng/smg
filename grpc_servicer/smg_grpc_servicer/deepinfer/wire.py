"""What the engine-core process and the gRPC frontend exchange.

The two run the same code on one host and talk over local ZMQ IPC sockets,
so messages are pickled tuples:

* frontend → engine core, on the input socket:
  ``("add", Request)`` and ``("abort", [request_id, ...])``.
* engine core → frontend, on the output socket:
  ``("ready", ModelFacts)`` once the engine is built, or
  ``("failed", message)`` when building it fails;
  then ``("events", [(request_id, Delta | Finished), ...])`` per loop
  iteration, and ``("dead", message)`` if the step loop fails.

Nothing here imports gRPC, torch or DeepInfer.
"""

from __future__ import annotations

import os
import pickle
import tempfile
from dataclasses import dataclass, field
from typing import Any

ADD = "add"
ABORT = "abort"
READY = "ready"
FAILED = "failed"
EVENTS = "events"
DEAD = "dead"


@dataclass
class ModelFacts:
    """What the info RPCs report; read by the engine core once the engine is
    built and sent with its ``ready`` message."""

    model_path: str
    model_dir: str
    served_model_name: str
    max_model_len: int
    vocab_size: int
    model_type: str = ""
    architectures: list[str] = field(default_factory=list)
    eos_token_ids: list[int] = field(default_factory=list)
    pad_token_id: int = 0
    bos_token_id: int = 0
    engine_rank: int = 0
    expert_parallel_size: int = 1
    context_parallel_size: int = 1
    max_num_seqs: int = 0
    max_num_batched_tokens: int = 0


def encode(message: tuple[Any, ...]) -> bytes:
    return pickle.dumps(message, protocol=pickle.HIGHEST_PROTOCOL)


def decode(frame: bytes) -> tuple[Any, ...]:
    # Both ends are this package on one host (a private IPC socket).
    return pickle.loads(frame)  # noqa: S301


@dataclass(frozen=True)
class RankAddresses:
    """The IPC endpoints between one rank's frontend and engine core."""

    input: str
    output: str


def rank_addresses(dp_rank: int, run_dir: str | None = None) -> RankAddresses:
    """Fresh IPC endpoints for a rank, under ``run_dir`` (a new private
    directory by default)."""
    run_dir = run_dir or tempfile.mkdtemp(prefix="smg-deepinfer-")
    base = os.path.join(run_dir, f"dp{dp_rank}")
    return RankAddresses(input=f"ipc://{base}-in", output=f"ipc://{base}-out")
