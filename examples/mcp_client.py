"""A tiny MCP client for `openjersey mcp` over stdio (JSON-RPC, one object per line)."""
import base64, json, os, subprocess, sys

class Mcp:
    def __init__(self, exe, env=None, client_name="examples"):
        self.proc = subprocess.Popen([exe, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8", env=env, bufsize=1)
        self.next_id = 0
        self.request("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": client_name, "version": "1"}})

    def request(self, method, params=None):
        self.next_id += 1
        self.proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}}) + "\n")
        self.proc.stdin.flush()
        reply = json.loads(self.proc.stdout.readline())
        if "error" in reply:
            raise RuntimeError(reply["error"])
        return reply["result"]

    def call(self, tool, **args):
        """Returns (parsed text result, [png bytes]); raises on a tool error."""
        result = self.request("tools/call", {"name": tool, "arguments": args})
        text = "".join(c["text"] for c in result["content"] if c["type"] == "text")
        if result.get("isError"):
            raise RuntimeError(f"{tool}: {text}")
        images = [base64.b64decode(c["data"]) for c in result["content"] if c["type"] == "image"]
        try:
            return json.loads(text), images
        except ValueError:
            return text, images

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=10)
