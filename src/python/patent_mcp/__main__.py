"""Entry point: python -m patent_mcp"""
import argparse
import sys

from patent_mcp.http_transport import DEFAULT_HTTP_HOST, DEFAULT_HTTP_PORT


def main() -> None:
    parser = argparse.ArgumentParser(
        prog="patent-mcp-server",
        description="MCP server for fetching and caching patents by ID",
    )
    parser.add_argument("--cache-dir", default=None, help="Local cache directory (default: ~/.local/share/patent-cache/patents)")
    parser.add_argument("--log-level", default="info", choices=["debug", "info", "warn", "error"])

    subparsers = parser.add_subparsers(dest="command")

    # canonicalize subcommand: print canonical JSON for a patent ID
    can_p = subparsers.add_parser("canonicalize", help="Canonicalize a patent ID and print JSON")
    can_p.add_argument("id", help="Patent ID (e.g. US7654321, EP1234567)")

    # fetch-one subcommand: fetch a single patent and print JSON result
    fetch_p = subparsers.add_parser("fetch-one", help="Fetch a single patent and print JSON result")
    fetch_p.add_argument("id", help="Patent ID to fetch")

    http_p = subparsers.add_parser("serve-http", help="Run the MCP server over localhost Streamable HTTP")
    http_p.add_argument("--host", default=DEFAULT_HTTP_HOST)
    http_p.add_argument("--port", type=int, default=DEFAULT_HTTP_PORT)

    args = parser.parse_args()

    if args.command == "canonicalize":
        import json
        import dataclasses
        from patent_mcp.id_canon import canonicalize
        result = canonicalize(args.id)
        print(json.dumps(dataclasses.asdict(result)))
        return

    if args.command == "fetch-one":
        import asyncio
        import json
        from patent_mcp.config import load_config
        from patent_mcp.cache import PatentCache
        from patent_mcp.fetchers.orchestrator import FetcherOrchestrator
        from patent_mcp.id_canon import canonicalize

        overrides = {}
        if args.cache_dir:
            from pathlib import Path
            overrides["cache_local_dir"] = Path(args.cache_dir)

        config = load_config(overrides=overrides)
        cache = PatentCache(config)
        orc = FetcherOrchestrator(config, cache)
        canon = canonicalize(args.id)

        async def _run():
            import tempfile, pathlib
            out_dir = pathlib.Path(config.cache_local_dir) / canon.canonical
            result = await orc.fetch(canon, out_dir)
            files = {k: str(v) for k, v in result.files.items()}
            meta = None
            if result.metadata:
                import dataclasses
                meta = dataclasses.asdict(result.metadata)
            sources = []
            for s in result.sources:
                sources.append({
                    "source": s.source,
                    "success": s.success,
                    "elapsed_ms": s.elapsed_ms,
                    "error": s.error,
                })
            print(json.dumps({
                "canonical_id": result.canonical_id,
                "success": result.success,
                "from_cache": result.from_cache,
                "files": files,
                "metadata": meta,
                "sources": sources,
                "error": result.error,
            }))

        asyncio.run(_run())
        return

    if args.command == "serve-http":
        from patent_mcp.server import run_http

        run_http(
            cache_dir=args.cache_dir,
            log_level=args.log_level,
            host=args.host,
            port=args.port,
        )
        return

    # Default: run MCP server
    from patent_mcp.server import run_server
    run_server(cache_dir=args.cache_dir, log_level=args.log_level)


if __name__ == "__main__":
    main()
