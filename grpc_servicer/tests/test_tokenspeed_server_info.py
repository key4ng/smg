"""The TokenSpeed Python servicer's ``GetServerInfo`` carries the engine's
reasoning parser and where it starts a request's grammar on a thinking prompt
as ``server_args`` keys, the Router's label source for this engine."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

import pytest

pytest.importorskip("msgspec")
pytest.importorskip("zmq")
pytest.importorskip("numpy")
pytest.importorskip("smg_grpc_proto")
from smg_grpc_proto.generated import tokenspeed_scheduler_pb2  # noqa: E402


def _server_info(tokenspeed_servicer_mod, monkeypatch, **server_args):
    monkeypatch.setattr(tokenspeed_servicer_mod, "_engine_supports_dp_rank_pin", lambda: False)
    monkeypatch.delenv("SMG_PAIRING_PROTOCOL", raising=False)
    cls = tokenspeed_servicer_mod.TokenSpeedSchedulerServicer
    servicer = cls.__new__(cls)
    servicer.server_args = SimpleNamespace(
        **{"model": "m", "mapping": SimpleNamespace(attn=SimpleNamespace(dp_size=1)), **server_args}
    )
    servicer.scheduler_info = {}
    servicer.start_time = 0.0
    servicer.async_llm = SimpleNamespace(rid_to_state={})
    return asyncio.run(
        servicer.GetServerInfo(tokenspeed_scheduler_pb2.GetServerInfoRequest(), None)
    )


def test_server_info_carries_where_the_engine_starts_a_grammar(
    tokenspeed_servicer_mod, monkeypatch
):
    # This engine applies a grammar from the first generated token, with or
    # without a reasoning parser, so the Router sends the thought and then the
    # calls; the parser rides along under its own name.
    gated = _server_info(
        tokenspeed_servicer_mod, monkeypatch, reasoning_parser="kimi_k25"
    ).server_args
    assert gated["constrained_decoding_start"] == "first_token"
    assert gated["engine_reasoning_parser"] == "kimi_k25"
    # The engine's own argument keeps its name; it is not the Router's parser
    # override, which the Router reads under `reasoning_parser` from labels.
    assert gated["reasoning_parser"] == "kimi_k25"

    plain = _server_info(tokenspeed_servicer_mod, monkeypatch, reasoning_parser=None).server_args
    assert plain["constrained_decoding_start"] == "first_token"
    assert "engine_reasoning_parser" not in plain
    assert plain["model"] == "m", "the other server args are reported as before"


def test_server_info_reports_the_engines_constrained_decoding_policy(
    tokenspeed_servicer_mod, monkeypatch
):
    """The engine's declared policy (a grammar on every turn) reaches the
    gateway as a field of its own, spelled as the `constrained_decoding` label
    reads it; an engine that declares none leaves the field at its default."""
    always = _server_info(tokenspeed_servicer_mod, monkeypatch, constrained_decoding="ALWAYS")
    assert always.constrained_decoding == "always"
    requested = _server_info(tokenspeed_servicer_mod, monkeypatch, constrained_decoding="requested")
    assert requested.constrained_decoding == "requested"
    silent = _server_info(tokenspeed_servicer_mod, monkeypatch)
    assert silent.constrained_decoding == ""
    unknown = _server_info(tokenspeed_servicer_mod, monkeypatch, constrained_decoding="sometimes")
    assert unknown.constrained_decoding == ""


def test_the_resolved_tp_width_is_advertised_when_the_field_is_unset(
    tokenspeed_servicer_mod, monkeypatch
):
    # TokenSpeed keeps the resolved width on ``mapping``; the gateway reads
    # ``attn_tp_size`` as the worker's ``tp_size``.
    info = _server_info(
        tokenspeed_servicer_mod,
        monkeypatch,
        attn_tp_size=None,
        mapping=SimpleNamespace(attn=SimpleNamespace(tp_size=2, dp_size=1)),
    )
    assert info.server_args["attn_tp_size"] == 2


def test_no_tp_width_is_advertised_when_the_engine_gives_none(tokenspeed_servicer_mod, monkeypatch):
    info = _server_info(tokenspeed_servicer_mod, monkeypatch, attn_tp_size=None)
    assert "attn_tp_size" not in info.server_args


def test_no_tp_width_is_advertised_under_attention_dp(tokenspeed_servicer_mod, monkeypatch):
    # The command-line field is the attention width too, so it does not pass through.
    info = _server_info(
        tokenspeed_servicer_mod,
        monkeypatch,
        attn_tp_size=2,
        mapping=SimpleNamespace(attn=SimpleNamespace(tp_size=2, dp_size=4)),
    )
    assert "attn_tp_size" not in info.server_args
