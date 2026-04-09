from __future__ import annotations

import json
from typing import Any

from starlette.applications import Starlette
from starlette.requests import Request
from starlette.responses import JSONResponse, Response
from starlette.routing import Route
import uvicorn

DEFAULT_HTTP_HOST = "127.0.0.1"
DEFAULT_HTTP_PORT = 38473
MCP_HTTP_PATH = "/mcp"
MCP_PROTOCOL_VERSION = "2024-11-05"


def _rpc_ok(rpc_id: Any, result: Any) -> dict[str, Any]:
    return {
        "jsonrpc": "2.0",
        "id": rpc_id,
        "result": result,
    }


def _rpc_err(rpc_id: Any, code: int, message: str) -> dict[str, Any]:
    return {
        "jsonrpc": "2.0",
        "id": rpc_id,
        "error": {
            "code": code,
            "message": message,
        },
    }


def _initialize_result(name: str, params: dict[str, Any] | None) -> dict[str, Any]:
    protocol_version = MCP_PROTOCOL_VERSION
    if isinstance(params, dict) and params.get("protocolVersion") == MCP_PROTOCOL_VERSION:
        protocol_version = params["protocolVersion"]

    result = {
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": name, "version": "0.1.0"},
    }
    return result


def _serialize_tool(tool: Any) -> dict[str, Any]:
    payload = tool.model_dump(exclude_none=True) if hasattr(tool, "model_dump") else dict(tool)
    result = {
        "name": payload["name"],
        "description": payload.get("description", ""),
        "inputSchema": payload.get("inputSchema", {"type": "object", "properties": {}}),
    }
    if "annotations" in payload:
        result["annotations"] = payload["annotations"]
    return result


def _serialize_content_block(block: Any) -> dict[str, Any]:
    if hasattr(block, "model_dump"):
        return block.model_dump(exclude_none=True)
    if isinstance(block, dict):
        return block
    text = getattr(block, "text", None)
    if text is not None:
        return {"type": "text", "text": text}
    return {"type": "text", "text": str(block)}


async def _handle_post(request: Request) -> Response:
    mcp = request.app.state.mcp

    try:
        payload = await request.json()
    except json.JSONDecodeError as exc:
        return JSONResponse(_rpc_err(None, -32700, f"Parse error: {exc}"))

    rpc_id = payload.get("id")
    method = payload.get("method")
    params = payload.get("params")

    if method == "initialize":
        name = getattr(mcp, "name", None) or "patent-mcp-server"
        return JSONResponse(_rpc_ok(rpc_id, _initialize_result(name, params)))

    if method in {"initialized", "notifications/initialized"}:
        return Response(status_code=202)

    if method == "ping":
        return JSONResponse(_rpc_ok(rpc_id, {}))

    if method == "tools/list":
        tools = await mcp.list_tools()
        return JSONResponse(_rpc_ok(rpc_id, {"tools": [_serialize_tool(tool) for tool in tools]}))

    if method == "tools/call":
        if not isinstance(params, dict):
            return JSONResponse(_rpc_err(rpc_id, -32602, "Missing params"))

        name = params.get("name")
        if not isinstance(name, str) or not name:
            return JSONResponse(_rpc_err(rpc_id, -32602, "Missing tool name"))

        arguments = params.get("arguments")
        if arguments is None:
            arguments = {}
        if not isinstance(arguments, dict):
            return JSONResponse(_rpc_err(rpc_id, -32602, "Invalid arguments"))

        try:
            result = await mcp.call_tool(name, arguments)
        except Exception as exc:
            return JSONResponse(
                _rpc_ok(
                    rpc_id,
                    {
                        "content": [{"type": "text", "text": str(exc)}],
                        "isError": True,
                    },
                )
            )

        if isinstance(result, dict):
            is_error = bool(result.get("isError", False))
            text = json.dumps(result)
            return JSONResponse(
                _rpc_ok(
                    rpc_id,
                    {
                        "content": [{"type": "text", "text": text}],
                        "isError": is_error,
                    },
                )
            )

        return JSONResponse(
            _rpc_ok(
                rpc_id,
                {
                    "content": [_serialize_content_block(block) for block in result],
                    "isError": False,
                },
            )
        )

    return JSONResponse(_rpc_err(rpc_id, -32601, "Method not found"))


async def _method_not_allowed(_request: Request) -> Response:
    return Response(status_code=405)


def build_http_app(mcp) -> Starlette:
    app = Starlette(
        routes=[
            Route(MCP_HTTP_PATH, _handle_post, methods=["POST"]),
            Route(MCP_HTTP_PATH, _method_not_allowed, methods=["GET", "DELETE"]),
        ]
    )
    app.state.mcp = mcp
    return app


def run_http_server(
    mcp,
    host: str = DEFAULT_HTTP_HOST,
    port: int = DEFAULT_HTTP_PORT,
    log_level: str = "info",
) -> None:
    uvicorn.run(
        build_http_app(mcp),
        host=host,
        port=port,
        log_level=log_level.lower().replace("warn", "warning"),
    )
