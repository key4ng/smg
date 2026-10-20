"""CLI entrypoint: ``python -m smg_grpc_servicer.deepinfer --model <model> --port 50051``.

See :mod:`smg_grpc_servicer.deepinfer.server` for the flags.
"""

from smg_grpc_servicer.deepinfer.server import main

if __name__ == "__main__":
    main()
