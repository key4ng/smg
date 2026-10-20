"""Standard gRPC health check for the DeepInfer server, on the frame the engine
servicers share (:mod:`smg_grpc_servicer.health_frame`)."""

from __future__ import annotations

import logging

from smg_grpc_servicer.health_frame import NOT_SERVING, SERVING, HealthFrame

logger = logging.getLogger(__name__)


class DeepInferHealthServicer(HealthFrame):
    """SERVING once the server is up, while the engine core runs."""

    ENGINE_SERVICE = "deepinfer.grpc.engine.DeepInferEngine"

    def __init__(self, engine):
        self._engine = engine
        self._serving = False

    def set_serving(self):
        self._serving = True
        logger.info("Health status set to SERVING")

    def set_not_serving(self):
        self._serving = False
        logger.info("Health status set to NOT_SERVING")

    def shutting_down(self) -> bool:
        return not self._serving

    async def engine_status(self, service_name: str) -> int:
        return SERVING if self._serving and self._engine.is_alive() else NOT_SERVING
