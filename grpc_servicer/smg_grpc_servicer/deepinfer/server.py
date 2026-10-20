"""DeepInfer gRPC server: a launcher, an engine core and a frontend per rank.

CLI::

    python -m smg_grpc_servicer.deepinfer --model <path-or-hf-id> --port 50051 \\
        --data-parallel-size 4 --expert-parallel-size 4

DeepInfer runs data parallelism as one engine per rank, all stepping in
lockstep. For each local rank this launcher spawns two processes, as vLLM
splits its ``AsyncLLM`` frontend from its ``EngineCoreProc``:

* the engine core (:mod:`.engine_core`) owns the GPUs and runs the step loop;
* the frontend serves gRPC on ``--port + r`` and talks to its engine core over
  local ZMQ IPC (:mod:`.engine_client`), so request handling and protobuf
  encoding never compete with the step loop for the interpreter.

Each rank is its own gateway worker (register every port), and the gateway's
load balancing spreads requests across them. The first process to exit stops
all the others. Ranks
are split across nodes the way DeepInfer's own launcher splits them, by
``--node-rank`` / ``--nnodes`` (default: the ``RANK`` / ``WORLD_SIZE``
environment). ``MASTER_ADDR`` / ``MASTER_PORT`` set DeepInfer's rendezvous.

DeepInfer's feature switches (``DEEPINFER_USE_NEXTN_DECODE`` and the rest)
are environment variables and pass through unchanged.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import multiprocessing
import os
import shutil
import signal
import sys
import tempfile
import time
from concurrent import futures
from pathlib import Path

from smg_grpc_servicer.deepinfer import wire

logger = logging.getLogger(__name__)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="DeepInfer gRPC inference server")
    parser.add_argument("--model", required=True, help="Local HF checkpoint dir or HF model id")
    parser.add_argument("--served-model-name", default=None)
    parser.add_argument("--host", default="0.0.0.0", help="gRPC listen address")
    parser.add_argument("--port", type=int, default=50051, help="gRPC port of DP rank 0")
    parser.add_argument("--data-parallel-size", type=int, default=1)
    parser.add_argument("--expert-parallel-size", type=int, default=1)
    parser.add_argument("--context-parallel-size", type=int, default=1)
    parser.add_argument("--node-rank", type=int, default=int(os.environ.get("RANK", "0")))
    parser.add_argument("--nnodes", type=int, default=int(os.environ.get("WORLD_SIZE", "1")))
    parser.add_argument("--max-model-len", type=int, default=None)
    parser.add_argument("--max-num-seqs", type=int, default=256)
    parser.add_argument("--max-num-batched-tokens", type=int, default=None)
    parser.add_argument("--gpu-memory-utilization", type=float, default=0.9)
    parser.add_argument(
        "--lookahead-tokens",
        type=int,
        default=None,
        help="Default: 1 while DEEPINFER_USE_NEXTN_DECODE (default 5) is on, else 0, "
        "as DeepInfer's examples set it",
    )
    parser.add_argument(
        "--no-chunked-prefill",
        dest="chunked_prefill",
        action="store_false",
        help="Disable chunked prefill (on by default, as in DeepInfer's examples)",
    )
    parser.add_argument(
        "--stop-preempted-request",
        action="store_true",
        help="Fail a request the engine preempts for KV space instead of requeueing it",
    )
    parser.add_argument("--trust-remote-code", action="store_true")
    parser.add_argument("--seed", type=int, default=0)
    args = parser.parse_args(argv)
    if args.data_parallel_size % args.nnodes:
        parser.error("--data-parallel-size must be a multiple of --nnodes")
    if args.lookahead_tokens is None:
        args.lookahead_tokens = min(1, int(os.environ.get("DEEPINFER_USE_NEXTN_DECODE", "5")))
    return args


def local_ranks(dp_size: int, node_rank: int, nnodes: int) -> list[int]:
    """The DP ranks this node runs: a contiguous block, as DeepInfer's
    ``run_dp`` assigns them."""
    per_node = dp_size // nnodes
    return list(range(node_rank * per_node, (node_rank + 1) * per_node))


def visible_devices(dp_rank: int, cp_size: int, device_count: int, offset: int) -> str:
    """``CUDA_VISIBLE_DEVICES`` for a rank: its ``cp_size`` GPUs, as DeepInfer's
    ``setup_cuda_visible_devices`` computes them (``DEVICE_OFFSET`` shifts the
    first one)."""
    local = dp_rank * cp_size % device_count // cp_size
    return ",".join(str(offset + i) for i in range(local * cp_size, (local + 1) * cp_size))


def resolve_model_dir(model: str) -> str:
    path = Path(model).expanduser()
    if path.is_dir():
        return str(path.resolve())
    from huggingface_hub import snapshot_download

    return snapshot_download(model)


def _model_facts(engine, args: argparse.Namespace, model_dir: str, dp_rank: int):
    config: dict = {}
    config_path = Path(model_dir) / "config.json"
    if config_path.is_file():
        config = json.loads(config_path.read_text())
    tokenizer = engine.tokenizer
    eos = config.get("eos_token_id", tokenizer.eos_token_id)
    eos_ids = [eos] if isinstance(eos, int) else [int(e) for e in eos or []]
    model_config = engine.model_config
    return wire.ModelFacts(
        model_path=args.model,
        model_dir=model_dir,
        served_model_name=args.served_model_name or args.model,
        max_model_len=int(model_config.max_model_len),
        vocab_size=int(model_config.get_vocab_size()),
        model_type=config.get("model_type", ""),
        architectures=list(config.get("architectures", [])),
        eos_token_ids=eos_ids,
        pad_token_id=int(tokenizer.pad_token_id or 0),
        bos_token_id=int(tokenizer.bos_token_id or 0),
        engine_rank=dp_rank,
        expert_parallel_size=args.expert_parallel_size,
        context_parallel_size=args.context_parallel_size,
        max_num_seqs=int(model_config.max_num_seqs),
        max_num_batched_tokens=int(model_config.max_num_batched_tokens),
    )


def _build_engine(args: argparse.Namespace, dp_rank: int, model_dir: str):
    # Imported here: DeepInfer reads its environment (CUDA_VISIBLE_DEVICES
    # among it) at import time.
    from deepinfer.engine.arg_utils import EngineArgs
    from deepinfer.engine.llm_engine import LLMEngine

    engine_args = EngineArgs(
        model=model_dir,
        served_model_name=args.served_model_name or args.model,
        trust_remote_code=args.trust_remote_code,
        seed=args.seed,
        max_model_len=args.max_model_len,
        data_parallel_size=args.data_parallel_size,
        context_parallel_size=args.context_parallel_size,
        expert_parallel_size=args.expert_parallel_size,
        engine_rank=dp_rank,
        gpu_memory_utilization=args.gpu_memory_utilization,
        max_num_batched_tokens=args.max_num_batched_tokens,
        max_num_seqs=args.max_num_seqs,
        chunked_prefill=args.chunked_prefill,
        lookahead_tokens=args.lookahead_tokens,
        stop_preempted_request=args.stop_preempted_request,
    )
    return LLMEngine.from_engine_args(engine_args)


def _clone_output(ptr):
    from deepinfer._C.engine import RequestOutput

    return RequestOutput.clone(int(ptr))


def _engine_core_main(
    args: argparse.Namespace,
    dp_rank: int,
    model_dir: str,
    devices: str,
    addresses: wire.RankAddresses,
) -> None:
    """Entry of a rank's engine-core process: the GPU side."""
    os.environ["CUDA_VISIBLE_DEVICES"] = devices
    if os.environ.get("DEEPINFER_ACCELERATOR") == "ascend":
        os.environ["ASCEND_RT_VISIBLE_DEVICES"] = devices
    os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
    _setup_logging(f"dp{dp_rank}-engine")

    from smg_grpc_servicer.deepinfer.engine_core import EngineCore
    from smg_grpc_servicer.deepinfer.engine_loop import EngineLoop

    engine_loop = EngineLoop(lambda: _build_engine(args, dp_rank, model_dir), _clone_output)
    core = EngineCore(
        engine_loop,
        addresses,
        lambda engine: _model_facts(engine, args, model_dir, dp_rank),
    )
    for sig in (signal.SIGTERM, signal.SIGINT):
        signal.signal(sig, lambda _signum, _frame: core.stop())
    sys.exit(core.run())


async def serve_frontend(
    args: argparse.Namespace,
    dp_rank: int,
    addresses: wire.RankAddresses,
    engine_pid: int | None,
) -> None:
    """Wait for the rank's engine core, then serve gRPC until SIGTERM/SIGINT."""
    import grpc
    from grpc_health.v1 import health_pb2_grpc
    from grpc_reflection.v1alpha import reflection
    from smg_grpc_proto import deepinfer_engine_pb2, deepinfer_engine_pb2_grpc

    from smg_grpc_servicer.deepinfer.engine_client import EngineClient
    from smg_grpc_servicer.deepinfer.health_servicer import DeepInferHealthServicer
    from smg_grpc_servicer.deepinfer.servicer import DeepInferEngineServicer
    from smg_grpc_servicer.hostport import host_port

    start_time = time.time()
    loop = asyncio.get_running_loop()
    stop_event = asyncio.Event()
    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(sig, stop_event.set)

    client = EngineClient(addresses, engine_pid)
    startup = asyncio.create_task(client.start())
    stopping = asyncio.create_task(stop_event.wait())
    await asyncio.wait({startup, stopping}, return_when=asyncio.FIRST_COMPLETED)
    if not startup.done():
        startup.cancel()
        await client.close()
        return
    stopping.cancel()
    facts = startup.result()  # raises EngineStartupError: the process exits non-zero

    server = grpc.aio.server(
        futures.ThreadPoolExecutor(max_workers=10),
        options=[
            ("grpc.max_send_message_length", 1024 * 1024 * 256),
            ("grpc.max_receive_message_length", 1024 * 1024 * 256),
            ("grpc.http2.min_ping_interval_without_data_ms", 10000),
            ("grpc.keepalive_permit_without_calls", True),
        ],
    )
    health = DeepInferHealthServicer(client)
    health_pb2_grpc.add_HealthServicer_to_server(health, server)
    servicer = DeepInferEngineServicer(client, facts, start_time)
    deepinfer_engine_pb2_grpc.add_DeepInferEngineServicer_to_server(servicer, server)
    reflection.enable_server_reflection(
        (
            deepinfer_engine_pb2.DESCRIPTOR.services_by_name["DeepInferEngine"].full_name,
            "grpc.health.v1.Health",
            reflection.SERVICE_NAME,
        ),
        server,
    )

    listen_addr = host_port(args.host, args.port + dp_rank)
    if server.add_insecure_port(listen_addr) == 0:
        raise RuntimeError(f"Failed to bind gRPC server to {listen_addr}")
    await server.start()
    health.set_serving()
    logger.info("DeepInfer DP rank %d serving on %s (model %s)", dp_rank, listen_addr, args.model)
    try:
        await stop_event.wait()
    finally:
        logger.info("DP rank %d frontend shutting down", dp_rank)
        health.set_not_serving()
        servicer.set_shutting_down()
        await server.stop(5.0)
        await client.close()


def _frontend_main(
    args: argparse.Namespace,
    dp_rank: int,
    addresses: wire.RankAddresses,
    engine_pid: int,
) -> None:
    """Entry of a rank's frontend process: gRPC, no GPU."""
    _setup_logging(f"dp{dp_rank}-frontend")
    asyncio.run(serve_frontend(args, dp_rank, addresses, engine_pid))


def _setup_logging(tag: str) -> None:
    logging.basicConfig(
        level=logging.INFO,
        format=f"%(asctime)s %(levelname)s [{tag}] %(name)s: %(message)s",
    )


def _device_count() -> int:
    visible = os.environ.get("CUDA_VISIBLE_DEVICES")
    if visible:
        return len([d for d in visible.split(",") if d.strip()])
    import torch

    return torch.cuda.device_count()


def main(argv: list[str] | None = None) -> None:
    _setup_logging("launcher")
    args = parse_args(argv)
    model_dir = resolve_model_dir(args.model)
    ranks = local_ranks(args.data_parallel_size, args.node_rank, args.nnodes)
    device_count = _device_count()
    offset = int(os.environ.get("DEVICE_OFFSET", "0"))
    plan = {r: visible_devices(r, args.context_parallel_size, device_count, offset) for r in ranks}
    logger.info("Launching DeepInfer DP ranks %s (devices %s)", ranks, plan)

    run_dir = tempfile.mkdtemp(prefix="smg-deepinfer-")
    ctx = multiprocessing.get_context("spawn")
    procs: dict[str, multiprocessing.process.BaseProcess] = {}
    try:
        for r in ranks:
            addresses = wire.rank_addresses(r, run_dir)
            core = ctx.Process(
                target=_engine_core_main,
                args=(args, r, model_dir, plan[r], addresses),
                name=f"deepinfer-dp{r}-engine",
            )
            core.start()
            frontend = ctx.Process(
                target=_frontend_main,
                args=(args, r, addresses, core.pid),
                name=f"deepinfer-dp{r}-frontend",
            )
            frontend.start()
            procs[core.name] = core
            procs[frontend.name] = frontend
        sys.exit(_supervise(procs))
    finally:
        shutil.rmtree(run_dir, ignore_errors=True)


#: How long the processes get to exit on SIGTERM before they are killed. An
#: engine core still building its engine (blocked in DeepInfer's distributed
#: init, say) does not act on SIGTERM at all.
STOP_GRACE_S = 30.0


def _supervise(procs: dict[str, multiprocessing.process.BaseProcess]) -> int:
    """Wait on every process; the first to exit stops the rest. DP ranks step
    in lockstep (a dead rank leaves the others blocked in a collective), and a
    frontend is useless without its engine core and vice versa. Processes still
    alive ``STOP_GRACE_S`` after the stop are killed, and so is everything they
    started (DeepInfer's multimodal processor workers, among others)."""
    _become_subreaper()
    stop_deadline: float | None = None

    def stop_all(signum=signal.SIGTERM, _frame=None):
        nonlocal stop_deadline
        if stop_deadline is None:
            stop_deadline = time.monotonic() + STOP_GRACE_S
        for p in procs.values():
            if p.is_alive():
                os.kill(p.pid, signum)

    signal.signal(signal.SIGTERM, stop_all)
    signal.signal(signal.SIGINT, stop_all)

    exit_code = 0
    live = dict(procs)
    while live:
        for name, p in list(live.items()):
            p.join(timeout=0.5)
            if p.exitcode is None:
                continue
            del live[name]
            if stop_deadline is None:
                logger.error("%s exited with %s; stopping the others", name, p.exitcode)
                exit_code = 1
                stop_all()
            elif p.exitcode not in (0, -signal.SIGTERM, -signal.SIGINT, -signal.SIGKILL):
                exit_code = 1
        if live and stop_deadline is not None and time.monotonic() > stop_deadline:
            logger.warning(
                "Killing %s, still running %.0fs after the stop", list(live), STOP_GRACE_S
            )
            for p in live.values():
                _kill_tree(p.pid)
    for pid in _descendants(os.getpid()):
        _kill(pid)
    _reap_orphans()
    return exit_code


def _become_subreaper() -> None:
    """Make processes orphaned under this launcher (the children of a killed
    engine core) its children, so that :func:`_descendants` still finds them."""
    if not sys.platform.startswith("linux"):
        return
    import ctypes

    pr_set_child_subreaper = 36
    try:
        ctypes.CDLL(None, use_errno=True).prctl(pr_set_child_subreaper, 1, 0, 0, 0)
    except (OSError, AttributeError):
        logger.debug("prctl(PR_SET_CHILD_SUBREAPER) unavailable", exc_info=True)


def _descendants(pid: int) -> list[int]:
    """Every live descendant of ``pid``, children first (Linux ``/proc``)."""
    children: dict[int, list[int]] = {}
    for entry in os.listdir("/proc") if os.path.isdir("/proc") else []:
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/stat") as f:
                # The command name may hold spaces; state and ppid follow its ")".
                state, ppid_field = f.read().rsplit(")", 1)[1].split()[:2]
            ppid = int(ppid_field)
        except (OSError, IndexError, ValueError):
            continue
        if state == "Z":
            continue
        children.setdefault(ppid, []).append(int(entry))
    found: list[int] = []
    stack = list(children.get(pid, []))
    while stack:
        child = stack.pop()
        found.append(child)
        stack.extend(children.get(child, []))
    return found


def _reap_orphans(timeout_s: float = 5.0) -> None:
    """Wait for the processes reparented to this subreaper, so none is left
    behind as a zombie."""
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        try:
            pid, _ = os.waitpid(-1, os.WNOHANG)
        except ChildProcessError:
            return
        if pid == 0:
            time.sleep(0.05)


def _kill_tree(pid: int) -> None:
    for child in _descendants(pid):
        _kill(child)
    _kill(pid)


def _kill(pid: int) -> None:
    try:
        os.kill(pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        pass


if __name__ == "__main__":
    main()
