import os, sys
from pathlib import Path
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))

from fakes import ScriptedBackend
from artifacts_mcp.errors import ArtifactsToolError
from artifacts_mcp.tools import build_server

class NoisyBackend(ScriptedBackend):
    def extract(self, file_path: str) -> str:
        print("STRAY STDOUT FROM A TOOL BODY")
        sys.stdout.flush()
        return super().extract(file_path)

def make_backend():
    mode = os.environ.get("WIRE_MODE", "ok")
    if mode == "boom": return ScriptedBackend(boom=RuntimeError("upstream 503"))
    if mode == "refused": return ScriptedBackend(boom=ArtifactsToolError("Unsupported file type."))
    if mode == "hang":
        class _Hanging(ScriptedBackend):
            def extract(self, file_path):
                import time; time.sleep(10)
                return "never"
        return _Hanging()
    if mode == "noisy": return NoisyBackend()
    return ScriptedBackend()

if __name__ == "__main__":
    timeout = float(os.environ.get("WIRE_TIMEOUT_SECS", "10"))
    build_server(make_backend(), timeout_secs=timeout).run("stdio")
