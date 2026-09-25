#!/usr/bin/env bash
# Regenerates include/dwell_net.h from the crate's C ABI. CI fails if the checked-in header is stale.
set -euo pipefail
cd "$(dirname "$0")"
cbindgen --config cbindgen.toml --crate dwell-net --output include/dwell_net.h
