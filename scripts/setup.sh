#!/usr/bin/env bash
# Development setup (debug tooling): everything needed to run the game in a browser and take
# screenshots (`cd client && npm run shot -- …`), idempotent — steps already done are skipped.
#   scripts/setup.sh            client deps, Emscripten, the WASM core, a client build
#   scripts/setup.sh --server   also the native dwell_server (the e2e suite's server)
#   scripts/setup.sh --e2e      also the local master server's deps (friend-world e2e)
# Emscripten is the version CI pins (.github/workflows/ci.yml), in ~/emsdk. Where www.sqlite.org
# is blocked (some sandboxes), the SQLite amalgamation comes from npm (CLAUDE.md, "Building
# without sqlite.org"). Prints the time each step took.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
SERVER=0
E2E=0
for arg in "$@"; do
  case "$arg" in
    --server) SERVER=1 ;;
    --e2e) E2E=1 ;;
    *) echo "unknown option $arg (--server, --e2e)" >&2; exit 2 ;;
  esac
done

step() { STEP=$1; STEP_T0=$(date +%s); echo "== $STEP"; }
done_step() { echo "   done in $(( $(date +%s) - STEP_T0 )) s"; }

# Client dependencies, when the lockfile changed since the last install.
step "client dependencies"
cd "$ROOT/client"
if [ ! -f node_modules/.package-lock.json ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
  npm ci --no-audit --no-fund
fi
done_step

if [ "$E2E" = 1 ]; then
  step "master server dependencies"
  cd "$ROOT/services/master"
  if [ ! -f node_modules/.package-lock.json ] || [ package-lock.json -nt node_modules/.package-lock.json ]; then
    npm ci --no-audit --no-fund
  fi
  done_step
fi

# Emscripten, the version CI installs.
step "Emscripten"
EMSDK_VERSION=$(grep -oE 'emsdk install [0-9.]+' "$ROOT/.github/workflows/ci.yml" | head -1 | awk '{print $3}')
export EMSDK="$HOME/emsdk"
if [ ! -d "$EMSDK" ]; then
  git clone --quiet --depth 1 https://github.com/emscripten-core/emsdk.git "$EMSDK"
fi
if ! "$EMSDK/emsdk" list 2>/dev/null | grep -qE "^\s+\*\s+$EMSDK_VERSION\s+INSTALLED|$EMSDK_VERSION.*INSTALLED"; then
  "$EMSDK/emsdk" install "$EMSDK_VERSION" >/dev/null
fi
"$EMSDK/emsdk" activate "$EMSDK_VERSION" >/dev/null
# shellcheck disable=SC1091
source "$EMSDK/emsdk_env.sh" >/dev/null 2>&1
done_step

# SQLite: from sqlite.org as CMake asks, or from npm where that host is blocked.
CMAKE_SQLITE=()
SQLITE_URL=$(grep -oE 'https://www\.sqlite\.org/[^ ]+\.zip' "$ROOT/server/cmake/Dependencies.cmake" | head -1)
if ! curl -fsSI --max-time 10 "$SQLITE_URL" >/dev/null 2>&1; then
  step "SQLite from npm (sqlite.org unreachable)"
  SQ_CACHE="$HOME/.cache/dwell/sqlite"
  if [ ! -f "$SQ_CACHE/package/deps/sqlite3/sqlite3.c" ]; then
    mkdir -p "$SQ_CACHE"
    (cd "$SQ_CACHE" && npm pack --silent better-sqlite3@13.0.3 >/dev/null && tar xzf better-sqlite3-*.tgz)
  fi
  WANT=$(echo "$SQLITE_URL" | grep -oE '[0-9]{7}')
  # The download's name encodes 3.53.4 as 3530400.
  HAVE=$(grep -m1 'define SQLITE_VERSION ' "$SQ_CACHE/package/deps/sqlite3/sqlite3.h" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' |
    awk -F. '{ printf "%d%02d%02d00", $1, $2, $3 }')
  if [ "$WANT" != "$HAVE" ]; then
    echo "   SQLite $HAVE from npm, but the build pins $WANT: pick the better-sqlite3 release that bundles it" >&2
    exit 1
  fi
  CMAKE_SQLITE=(-DFETCHCONTENT_SOURCE_DIR_SQLITE3="$SQ_CACHE/package/deps/sqlite3")
  done_step
fi

# The WASM core and worldgen (into client/public/wasm); incremental after the first build.
step "WASM build"
cd "$ROOT/server"
[ -f build/wasm/build.ninja ] || cmake --preset wasm "${CMAKE_SQLITE[@]}" >/dev/null
cmake --build --preset wasm >/dev/null
done_step

if [ "$SERVER" = 1 ]; then
  step "native server"
  [ -f build/dev/build.ninja ] || cmake --preset dev "${CMAKE_SQLITE[@]}" >/dev/null
  cmake --build --preset dev --target dwell_server >/dev/null
  done_step
fi

step "client build"
cd "$ROOT/client"
npx vite build >/dev/null
done_step

CHROMIUM=${DWELL_CHROMIUM:-}
[ -z "$CHROMIUM" ] && [ -x /opt/pw-browsers/chromium ] && CHROMIUM=/opt/pw-browsers/chromium
echo "== ready. Chromium: ${CHROMIUM:-none found (npx playwright install chromium)}"
echo "   e.g. cd client && npm run shot -- --at 0,80,0 --look 0,-10 --out shot.png"
