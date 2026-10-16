#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
[[ -f wifi.toml ]] || { echo 'Create wifi.toml with Wi-Fi credentials first.' >&2; exit 1; }
cargo --config wifi.toml build --release --locked
mkdir -p target/ota
espflash save-image --skip-update-check --chip esp32 --flash-size 4mb --flash-mode dio --flash-freq 40mhz \
  target/xtensa-esp32-none-elf/release/nekodoa target/ota/nekodoa.bin
python3 tools/check-artifacts.py application target/ota/nekodoa.bin
