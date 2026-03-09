#!/usr/bin/env python3
"""
Probe DeepSeek streaming API to see how tool_call arguments arrive chunk by chunk.
Usage: python3 probe_tool_stream.py <api_key>
"""

import json
import sys
import urllib.request

API_KEY = sys.argv[1] if len(sys.argv) > 1 else ""
URL = "https://api.deepseek.com/chat/completions"

payload = {
    "model": "deepseek-chat",
    "stream": True,
    "messages": [
        {
            "role": "user",
            "content": "Please call the write_file tool to write a short Python hello world script to /tmp/hello.py",
        }
    ],
    "tools": [
        {
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Write content to a file",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path"},
                        "content": {
                            "type": "string",
                            "description": "File content to write",
                        },
                    },
                    "required": ["path", "content"],
                },
            },
        }
    ],
    "tool_choice": "required",
}

req = urllib.request.Request(
    URL,
    data=json.dumps(payload).encode(),
    headers={
        "Authorization": f"Bearer {API_KEY}",
        "Content-Type": "application/json",
    },
    method="POST",
)

chunk_index = 0

print("=== RAW CHUNKS ===\n")

with urllib.request.urlopen(req) as resp:
    for raw_line in resp:
        line = raw_line.decode().strip()
        if not line or not line.startswith("data: "):
            continue
        data = line[6:]
        if data == "[DONE]":
            print("\n=== DONE ===")
            break

        try:
            chunk = json.loads(data)
        except json.JSONDecodeError:
            print(f"[parse error] {data}")
            continue

        choices = chunk.get("choices", [])
        if not choices:
            continue

        delta = choices[0].get("delta", {})
        finish_reason = choices[0].get("finish_reason")

        tool_calls = delta.get("tool_calls")
        content = delta.get("content")

        if tool_calls:
            for tc in tool_calls:
                idx = tc.get("index", 0)
                tc_id = tc.get("id")
                func = tc.get("function", {})
                name = func.get("name")
                args_delta = func.get("arguments", "")

                print(f"[chunk {chunk_index:03d}] tool_call[{idx}]", end="")
                if tc_id:
                    print(f"  id={tc_id!r}", end="")
                if name:
                    print(f"  name={name!r}", end="")
                if args_delta:
                    # Show the raw delta, repr so whitespace/newlines are visible
                    print(f"  args_delta={args_delta!r}", end="")
                print()

        elif content:
            print(f"[chunk {chunk_index:03d}] text: {content!r}")

        if finish_reason:
            print(f"[chunk {chunk_index:03d}] finish_reason={finish_reason!r}")

        chunk_index += 1
