#!/usr/bin/env python3
"""Fixture MCP server for M10's net-free tests.

Speaks newline-delimited MCP JSON-RPC over stdio, using only the Python
standard library (no external deps, so `cargo test` stays net-free). Implements
just enough of the MCP lifecycle for rmcp's client to initialize and drive it:

- `initialize`            -> responds with the server info + protocol version
- `notifications/initialized` -> ignored (no reply expected)
- `tools/list`            -> a fixed tool set: echo, add, uuid, fail_on_demand
- `tools/call`            -> echoes args / canned output / error, per tool
- an env-var `MOOSHK_CRASH_ON_TOOLS_CALL` switches the floodlight

The `crash` tool exits the process (for the reconnect test); a malformed tool
name returns a `-32602` JSON-RPC error; the `fail` tool returns
Framing: one JSON object per line (the MCP stdio / newline-delimited format).
"""

import json
import os
import sys
import time


SERVER_INFO = {
    "protocolVersion": "2024-11-05",
    "capabilities": {"tools": {}},
    "serverInfo": {"name": "mooshik-fixture", "version": "1.0.0"},
}

TOOL_SCHEMA = {"type": "object", "properties": {}, "additionalProperties": True}

TOOLS = [
    {
        "name": "echo",
        "description": "Echo the arguments back as JSON text.",
        "inputSchema": TOOL_SCHEMA,
    },
    {
        "name": "add",
        "description": "Add two numbers and return the sum.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "a": {"type": "number"},
                "b": {"type": "number"},
            },
            "required": ["a", "b"],
        },
    },
    {
        "name": "uuid",
        "description": "Return a fixed UUID.",
        "inputSchema": TOOL_SCHEMA,
    },
    {
        "name": "fail",
        "description": "Always return isError with text.",
        "inputSchema": TOOL_SCHEMA,
    },
    {
        "name": "crash",
        "description": "Exit the process (reconnect test).",
        "inputSchema": TOOL_SCHEMA,
    },
    {
        "name": "hang",
        "description": "Never answer the call (worker-bound test).",
        "inputSchema": TOOL_SCHEMA,
    },
]

# Protocol version negotiation: rmcp's client sends its supported versions in
# `initialize`; we accept the modern 2026-07-28 or fall back to 2025-06-18 /
# 2024-11-05. Respond with whatever the client proposed if we know it.
KNOWN_VERSIONS = {"2026-07-28", "2025-06-18", "2024-11-05", "2024-10-07"}


def emit(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()


def read_line():
    line = sys.stdin.readline()
    if not line:
        return None
    line = line.strip()
    if not line:
        return read_line()
    return json.loads(line)


def main():
    delay = os.environ.get("MOOSHIK_DELAY_MS")
    if delay:
        time.sleep(float(delay) / 1000.0)

    marker = os.environ.get("MOOSHIK_STDERR_MARKER")
    if marker:
        sys.stderr.write(marker + "\n")
        sys.stderr.flush()

    noise = int(os.environ.get("MOOSHIK_STDERR_BYTES") or 0)
    if noise:
        # Written before initialize is answered, so a host that never drains
        # stderr deadlocks the handshake once the pipe buffer fills.
        chunk = b"x" * 8192
        written = 0
        while written < noise:
            take = min(len(chunk), noise - written)
            sys.stderr.buffer.write(chunk[:take])
            sys.stderr.flush()
            written += take
        sys.stderr.write("\n")
        sys.stderr.flush()
    while True:
        msg = read_line()
        if msg is None:
            return
        method = msg.get("method")
        msg_id = msg.get("id")
        params = msg.get("params") or {}

        if method == "initialize":
            proto = params.get("protocolVersion")
            version = proto if proto in KNOWN_VERSIONS else "2024-11-05"
            emit({
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mooshik-fixture", "version": "1.0.0"},
                },
            })
        elif method == "notifications/initialized":
            # No reply expected.
            pass
        elif method == "tools/list":
            emit({
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {"tools": TOOLS},
            })
        elif method == "tools/call":
            name = (params.get("name") or "")
            arguments = params.get("arguments") or {}
            if name == "crash":
                sys.exit(3)
            if name == "hang":
                time.sleep(300)  # never answers the call
            elif name == "fail":
                emit({
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "content": [{"type": "text", "text": "boom: simulated failure"}],
                        "isError": True,
                    },
                })
            elif name == "echo":
                emit({
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "content": [{"type": "text", "text": json.dumps(arguments, sort_keys=True)}],
                    },
                })
            elif name == "add":
                a = arguments.get("a", 0)
                b = arguments.get("b", 0)
                emit({
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "content": [{"type": "text", "text": str(a + b)}],
                    },
                })
            elif name == "uuid":
                emit({
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "result": {
                        "content": [{"type": "text", "text": "00000000-0000-4000-8000-000000000001"}],
                    },
                })
            else:
                emit({
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "error": {"code": -32602, "message": f"unknown tool: {name}"},
                })
        elif method == "ping":
            emit({"jsonrpc": "2.0", "id": msg_id, "result": {}})
        else:
            # Unknown method.
            if msg_id is not None:
                emit({
                    "jsonrpc": "2.0",
                    "id": msg_id,
                    "error": {"code": -32601, "message": f"method not found: {method}"},
                })


if __name__ == "__main__":
    main()