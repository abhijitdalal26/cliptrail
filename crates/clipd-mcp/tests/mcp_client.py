#!/usr/bin/env python3
"""End-to-end check: real MCP SDK client -> clipd-mcp stdio server.

Usage: python3 crates/clipd-mcp/tests/mcp_client.py <clipd-mcp-binary> <data_dir>
Exits non-zero on failure.
"""
import asyncio
import json
import sys

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


async def main() -> int:
    binary, data_dir = sys.argv[1], sys.argv[2]
    params = StdioServerParameters(
        command=binary, args=["--data-dir", data_dir]
    )
    async with stdio_client(params) as (read, write):
        async with ClientSession(read, write) as session:
            init = await session.initialize()
            print("server:", init.server_info.name, init.server_info.version)

            tools = await session.list_tools()
            names = [t.name for t in tools.tools]
            print("tools:", names)
            assert len(names) == 8, names

            r = await session.call_tool(
                "search_clips", {"query": "cliptrail", "kind": "link"}
            )
            hits = json.loads(r.content[0].text)
            print("search hits:", len(hits))
            assert len(hits) == 1, hits
            clip_id = hits[0]["id"]

            r = await session.call_tool("get_clip", {"id": clip_id[:10]})
            clip = json.loads(r.content[0].text)
            assert "cliptrail" in clip["text"], clip

            # Sensitive clip: metadata only, no body.
            r = await session.call_tool(
                "search_clips", {"kind": "sensitive"}
            )
            sens = json.loads(r.content[0].text)
            assert sens and sens[0]["preview"] == "••••••", sens
            r = await session.call_tool("get_clip", {"id": sens[0]["id"]})
            masked = json.loads(r.content[0].text)
            assert "text" not in masked, masked

            r = await session.call_tool("clip_stats", {})
            stats = json.loads(r.content[0].text)
            print("stats:", stats)
            assert stats["total"] >= 4

            r = await session.call_tool("day_counts", {})
            assert json.loads(r.content[0].text)

            r = await session.call_tool(
                "pin_clip", {"id": clip_id, "pinned": True}
            )
            assert json.loads(r.content[0].text)["pinned"] is True

    print("MCP E2E: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
