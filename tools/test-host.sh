#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../tests/host"
# Clear the embedded target's -nostartfiles flag inherited from repository config.
RUSTFLAGS='' cargo +stable test --locked --target "$(rustc +stable -vV | sed -n 's/^host: //p')"
cd ../..
node tests/dashboard.mjs
