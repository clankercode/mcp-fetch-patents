from __future__ import annotations

import uvicorn

DEFAULT_HTTP_HOST = "127.0.0.1"
DEFAULT_HTTP_PORT = 38473
MCP_HTTP_PATH = "/mcp"


def run_http_server(
    mcp,
    host: str = DEFAULT_HTTP_HOST,
    port: int = DEFAULT_HTTP_PORT,
    log_level: str = "info",
) -> None:
    uvicorn.run(
        mcp.streamable_http_app(),
        host=host,
        port=port,
        log_level=log_level.lower().replace("warn", "warning"),
    )
