"""GetServerInfo advertises the engine's resolved TP width, which TokenSpeed
keeps on ``server_args.mapping`` and leaves off ``attn_tp_size`` unless the
command line sets it. The gateway reads ``attn_tp_size`` as the worker's
``tp_size``."""

import asyncio
from types import SimpleNamespace

import pytest

pytest.importorskip("smg_grpc_proto")
pytest.importorskip("zmq")
pytest.importorskip("msgspec")

from smg_grpc_proto.generated import tokenspeed_scheduler_pb2  # noqa: E402

servicer_mod = pytest.importorskip("smg_grpc_servicer.tokenspeed.servicer")
TokenSpeedSchedulerServicer = servicer_mod.TokenSpeedSchedulerServicer


def _info(server_args):
    servicer = TokenSpeedSchedulerServicer.__new__(TokenSpeedSchedulerServicer)
    servicer.server_args = server_args
    servicer.scheduler_info = {}
    servicer.start_time = 0.0
    servicer.async_llm = SimpleNamespace(rid_to_state={})
    return asyncio.run(
        servicer.GetServerInfo(tokenspeed_scheduler_pb2.GetServerInfoRequest(), None)
    )


def test_the_resolved_tp_width_is_advertised_when_the_field_is_unset():
    info = _info(
        SimpleNamespace(
            model="m",
            attn_tp_size=None,
            mapping=SimpleNamespace(attn=SimpleNamespace(tp_size=2, dp_size=1)),
        )
    )
    assert info.server_args["attn_tp_size"] == 2


def test_an_explicit_tp_width_is_advertised_as_given():
    info = _info(
        SimpleNamespace(
            model="m",
            attn_tp_size=4,
            mapping=SimpleNamespace(attn=SimpleNamespace(tp_size=4, dp_size=1)),
        )
    )
    assert info.server_args["attn_tp_size"] == 4
