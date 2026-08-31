#!/usr/bin/env sh
#
# Mooshik installer.
#
# Two things get installed, and they fail independently:
#
#   1. The `mooshik` binary. Required. A failure here fails the install.
#   2. The Python MCP servers (news, artifacts, coder) into a dedicated
#      virtualenv. Optional. A machine without a usable Python 3.10+ still
#      gets a working `mooshik`; it just cannot run those servers, and the
#      script says so and exits 0.
#
# Overrides, all optional:
#
#   INSTALL_DIR           where the binary goes            (default ~/.local/bin)
#   MOOSHIK_VENV_DIR      where the virtualenv goes
#                         (default $XDG_DATA_HOME/mooshik/venv,
#                          i.e. ~/.local/share/mooshik/venv)
#   MOOSHIK_SKIP_PYTHON=1 install the binary only
#   MOOSHIK_PYTHON        interpreter to build the virtualenv with (default python3)
#   MOOSHIK_VERSION       install this version instead of querying for the latest
#   MOOSHIK_BASE_URL      where the release assets live. Accepts file:///abs/dir,
#                         which is how a locally built release is tested before
#                         a tag exists. Implies you also set MOOSHIK_VERSION.
#
set -e

REPO="nrynss/mooshik"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# The MCP servers get their own virtualenv, never the system interpreter and
# never `pip install --user`. Mooshik pins its Python dependencies exactly
# (mcp==2.1.1, google-genai==2.20.0, google-adk==2.7.1); installing exact pins
# into a shared site-packages is how you silently break an unrelated project on
# the same machine. XDG-respecting, because this is application data, not
# configuration and not a binary.
MOOSHIK_DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/mooshik"
VENV_DIR="${MOOSHIK_VENV_DIR:-${MOOSHIK_DATA_DIR}/venv}"

# Set by install_python_servers(); read by the summary at the end.
PY_STATUS="failed"
PY_REASON="the Python step did not run."

detect_os() {
    OS="$(uname -s)"
    case "$OS" in
        Linux*)  echo "unknown-linux-gnu" ;;
        Darwin*) echo "apple-darwin" ;;
        *)       echo "unsupported" ;;
    esac
}

detect_arch() {
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64|amd64) echo "x86_64" ;;
        arm64|aarch64) echo "aarch64" ;;
        *)            echo "unsupported" ;;
    esac
}

OS_TARGET="$(detect_os)"
ARCH_TARGET="$(detect_arch)"

if [ "$OS_TARGET" = "unsupported" ] || [ "$ARCH_TARGET" = "unsupported" ]; then
    echo "Error: Unsupported operating system or architecture: $(uname -s) $(uname -m)" >&2
    exit 1
fi

TARGET="${ARCH_TARGET}-${OS_TARGET}"

if [ -n "${MOOSHIK_VERSION:-}" ]; then
    LATEST_TAG="$MOOSHIK_VERSION"
    case "$LATEST_TAG" in
        v*) ;;
        *)  LATEST_TAG="v${LATEST_TAG}" ;;
    esac
else
    echo "Detecting latest Mooshik release..."
    LATEST_TAG="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')"

    if [ -z "$LATEST_TAG" ]; then
        echo "Error: Failed to find latest release tag." >&2
        exit 1
    fi
fi

VERSION="${LATEST_TAG#v}"
BASE_URL="${MOOSHIK_BASE_URL:-https://github.com/${REPO}/releases/download/${LATEST_TAG}}"
ARCHIVE_NAME="mooshik-${VERSION}-${TARGET}.tar.gz"
# One asset for all platforms: these wheels are pure Python (py3-none-any), so
# there is nothing per-target about them. See .github/workflows/release.yml.
PYTHON_BUNDLE="mooshik-python-${VERSION}.tar.gz"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT INT TERM

# Verify one already-downloaded file in $TMP_DIR against checksums.txt.
# Returns non-zero on a real mismatch or a missing entry; a machine with no
# sha256 tool at all warns and passes, matching the previous behaviour.
#
# The status of the checking command is captured and returned by hand rather
# than left to `set -e`. This function is also called from inside an `if`
# condition (install_python_servers), where errexit is suspended for the whole
# call — a bare `sha256sum -c -` there would fail, fall through, and the
# function would still return 0. A checksum that only sometimes checks is worse
# than no checksum at all.
verify_checksum() {
    _file="$1"
    _line="$(grep -F "$_file" checksums.txt 2>/dev/null || true)"
    if [ -z "$_line" ]; then
        echo "Error: no checksum entry for ${_file} in checksums.txt" >&2
        return 1
    fi
    if command -v sha256sum >/dev/null 2>&1; then
        printf '%s\n' "$_line" | sha256sum -c - || return 1
    elif command -v shasum >/dev/null 2>&1; then
        printf '%s\n' "$_line" | shasum -a 256 -c - || return 1
    else
        echo "Warning: Neither sha256sum nor shasum found. Skipping checksum verification for ${_file}." >&2
    fi
    return 0
}

# ---------------------------------------------------------------- binary ----

echo "Downloading Mooshik ${LATEST_TAG} for ${TARGET}..."
curl -fsSL "${BASE_URL}/${ARCHIVE_NAME}" -o "${TMP_DIR}/${ARCHIVE_NAME}"
curl -fsSL "${BASE_URL}/checksums.txt" -o "${TMP_DIR}/checksums.txt"

echo "Verifying checksum..."
cd "$TMP_DIR"
verify_checksum "$ARCHIVE_NAME"

tar -xzf "$ARCHIVE_NAME"

mkdir -p "$INSTALL_DIR"
mv mooshik "$INSTALL_DIR/mooshik"
chmod +x "$INSTALL_DIR/mooshik"

# ------------------------------------------------------- python servers ----

# Everything below runs inside an `if` condition, which suspends `set -e` for
# the whole function body — so every step that can fail states its own failure
# explicitly and returns. That is the point: a missing Python must cost the
# user the MCP servers, not the install.
install_python_servers() {
    if [ "${MOOSHIK_SKIP_PYTHON:-0}" = "1" ]; then
        PY_REASON="MOOSHIK_SKIP_PYTHON=1 was set."
        return 1
    fi

    _py="${MOOSHIK_PYTHON:-python3}"
    if ! command -v "$_py" >/dev/null 2>&1; then
        PY_REASON="no '${_py}' interpreter was found on PATH."
        return 1
    fi
    _py="$(command -v "$_py")"

    if ! "$_py" -c 'import sys; raise SystemExit(0 if sys.version_info >= (3, 10) else 1)' >/dev/null 2>&1; then
        PY_REASON="${_py} is older than the required Python 3.10."
        return 1
    fi

    echo ""
    echo "Downloading the Mooshik MCP servers (${PYTHON_BUNDLE})..."
    # curl prints its own reason above this; do not restate it as a diagnosis.
    # A 404 (a release predating this asset) and a dropped connection are not
    # the same failure, and the script cannot tell them apart from here.
    if ! curl -fsSL "${BASE_URL}/${PYTHON_BUNDLE}" -o "${TMP_DIR}/${PYTHON_BUNDLE}"; then
        PY_REASON="${PYTHON_BUNDLE} could not be downloaded (see the curl error above)."
        return 1
    fi

    if ! verify_checksum "$PYTHON_BUNDLE"; then
        PY_REASON="the checksum for ${PYTHON_BUNDLE} did not verify."
        return 1
    fi

    _wheels="${TMP_DIR}/wheels"
    if ! mkdir -p "$_wheels" || ! tar -xzf "${TMP_DIR}/${PYTHON_BUNDLE}" -C "$_wheels"; then
        PY_REASON="${PYTHON_BUNDLE} could not be extracted."
        return 1
    fi

    # Re-runnable, and never destructive on the happy path: a virtualenv that
    # still works is reused and upgraded in place, so a second run is cheap and
    # cannot leave the user with less than they started with. Only a missing or
    # broken one is (re)built, and `--clear` makes that idempotent too.
    if [ -x "${VENV_DIR}/bin/python" ] && "${VENV_DIR}/bin/python" -c 'import sys' >/dev/null 2>&1; then
        echo "Reusing the existing virtualenv at ${VENV_DIR}..."
    else
        echo "Creating a virtualenv at ${VENV_DIR}..."
        if ! mkdir -p "$(dirname "$VENV_DIR")"; then
            PY_REASON="$(dirname "$VENV_DIR") could not be created."
            return 1
        fi
        if ! "$_py" -m venv --clear "$VENV_DIR"; then
            PY_REASON="'${_py} -m venv' failed. On Debian and Ubuntu the venv module ships separately: apt install python3-venv."
            return 1
        fi
    fi

    # --find-links so that mooshik-common==0.1.0 -- a dependency of all three
    # servers and a package that exists on no index -- resolves from the bundle
    # rather than being hunted for on PyPI. The servers' third-party pins
    # (mcp, google-genai, google-adk) do come from PyPI, so this step needs
    # network access.
    echo "Installing the MCP server packages and their pinned dependencies..."
    if ! "${VENV_DIR}/bin/python" -m pip install --quiet --upgrade \
            --find-links "$_wheels" "$_wheels"/*.whl; then
        PY_REASON="pip could not install the MCP server packages. Check network access to pypi.org."
        return 1
    fi

    for _script in mooshik-news-mcp mooshik-artifacts-mcp mooshik-coder-mcp; do
        if [ ! -x "${VENV_DIR}/bin/${_script}" ]; then
            PY_REASON="${_script} is missing from ${VENV_DIR}/bin after install."
            return 1
        fi
    done

    return 0
}

if install_python_servers; then
    PY_STATUS="ok"
    PY_REASON=""
else
    PY_STATUS="failed"
fi

# --------------------------------------------------------------- report ----

echo ""
echo "Mooshik installed successfully to ${INSTALL_DIR}/mooshik."

if [ "$PY_STATUS" = "ok" ]; then
    echo "MCP servers installed into ${VENV_DIR}:"
    echo "    ${VENV_DIR}/bin/mooshik-news-mcp"
    echo "    ${VENV_DIR}/bin/mooshik-artifacts-mcp"
    echo "    ${VENV_DIR}/bin/mooshik-coder-mcp"
else
    echo ""
    echo "The Python MCP servers were NOT installed: ${PY_REASON}"
    echo ""
    echo "Mooshik itself works. What you do not have without them:"
    echo "    news       web search and article grounding"
    echo "    artifacts  screenshot and audio ingestion into memory"
    echo "    coder      delegating repository edits to a coding agent"
    echo ""
    echo "To add them later, install Python 3.10 or newer and re-run:"
    echo "    curl -fsSL https://raw.githubusercontent.com/${REPO}/main/install.sh | sh"
    echo "Re-running is safe; it will not disturb the binary you already have."
fi

echo ""
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo "Note: ${INSTALL_DIR} is not in your PATH."
        echo "Add it to your shell configuration:"
        echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
        echo ""
        ;;
esac

if [ "$PY_STATUS" = "ok" ]; then
    cat <<EOF
To wire the servers up, add this to ~/.mooshik/config.toml (run 'mooshik init'
first if you have not). 'expose' is an allowlist -- a server with an empty one
is never spawned. Values under [mcp_servers.*.env] are vault SECRET NAMES, not
literal values; store each with 'mooshik secret set <name>'.

[permissions]
"mcp.news.*" = "prompt"
"mcp.artifacts.*" = "prompt"
"mcp.coder.*" = "prompt"

[mcp_servers.news]
command = "${VENV_DIR}/bin/mooshik-news-mcp"
expose = ["search_news", "fetch_article"]

[mcp_servers.news.env]
# Vertex: store your project id under the secret name 'gemini-project'.
# Developer API instead: swap this line for
#   MOOSHIK_GEMINI_API_KEY = "gemini-api-key"
MOOSHIK_GEMINI_PROJECT = "gemini-project"

[mcp_servers.artifacts]
command = "${VENV_DIR}/bin/mooshik-artifacts-mcp"
expose = ["extract_concepts"]

[mcp_servers.artifacts.env]
MOOSHIK_GEMINI_PROJECT = "gemini-project"

[mcp_servers.coder]
command = "${VENV_DIR}/bin/mooshik-coder-mcp"
# The agent name is an argument, not an env value: it is not a secret, and
# everything in the env table below is read as a vault secret NAME. One of
# claude, omp, cursor, agy.
args = ["--agent", "claude"]
expose = ["delegate", "check"]

[mcp_servers.coder.env]
ANTHROPIC_API_KEY = "anthropic-api-key"

The coder server does not contain a coding agent. It shells out to one, so the
CLI you name in --agent must be installed and authenticated separately
(Claude Code, OMP, Cursor Agent CLI, or Antigravity).
EOF
    echo ""
fi

echo "Run 'mooshik init' to initialize your workspace."
echo "Documentation: https://nrynss.github.io/mooshik/"
