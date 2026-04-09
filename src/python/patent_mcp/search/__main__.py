"""Allow running as: python -m patent_mcp.search"""

import argparse

from patent_mcp.http_transport import DEFAULT_HTTP_HOST, DEFAULT_HTTP_PORT
from patent_mcp.search.server import run, run_http


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="patent-search-mcp-server",
        description="MCP server for patent search and research sessions",
    )
    subparsers = parser.add_subparsers(dest="command")

    http_p = subparsers.add_parser("serve-http", help="Run the MCP server over localhost Streamable HTTP")
    http_p.add_argument("--host", default=DEFAULT_HTTP_HOST)
    http_p.add_argument("--port", type=int, default=DEFAULT_HTTP_PORT)

    args = parser.parse_args()

    if args.command == "serve-http":
        run_http(host=args.host, port=args.port)
        return

    run()


if __name__ == "__main__":
    main()
