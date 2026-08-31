from __future__ import annotations
import logging
import sys
from .backend import ArtifactsBackend, make_client
from mooshik_common.logs import quiet_library_logging

from .config import ConfigError, Settings
from .tools import build_server

log = logging.getLogger("artifacts_mcp")

def main(argv: list[str] | None = None) -> int:
    argv = sys.argv[1:] if argv is None else argv
    if argv:
        print_stderr("artifacts_mcp takes no arguments.")
        return 2

    try:
        settings = Settings.from_env()
    except ConfigError as error:
        configure_logging("INFO")
        log.error("configuration error: %s", error)
        return 2

    configure_logging(settings.log_level)
    log.info("artifacts MCP server starting: %s", settings.describe())

    try:
        client = make_client(settings)
    except Exception as error:
        log.error("could not build the Google client (%s)", type(error).__name__)
        log.debug("client construction failed", exc_info=True)
        return 2

    backend = ArtifactsBackend(
        client,
        model=settings.model,
        timeout_secs=settings.timeout_secs,
        secrets=(settings.api_key,) if settings.api_key else (),
    )
    build_server(backend, timeout_secs=settings.timeout_secs).run("stdio")
    return 0

LOG_FORMAT = "%(name)s %(levelname)s %(message)s"

def configure_logging(level: str) -> None:
    logging.basicConfig(level=logging.WARNING, stream=sys.stderr, format=LOG_FORMAT)
    quiet_library_logging()
    try:
        log.setLevel(level)
    except ValueError:
        log.setLevel(logging.INFO)
        log.warning("unrecognised ARTIFACTS_LOG_LEVEL %r; using INFO", level)

def print_stderr(message: str) -> None:
    sys.stderr.write(message + "\n")

if __name__ == "__main__":
    raise SystemExit(main())
