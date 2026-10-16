#!/usr/bin/env bash
# Cargo runner: always use the verified rollback bootloader for USB installs.
set -euo pipefail
elf="${1:?Missing ELF}"
shift
[[ "${1:-}" == --port && -n "${2:-}" && "$#" == 2 ]] || { echo 'Use cargo --config wifi.toml run --release --locked -- --port /dev/ttyUSB0' >&2; exit 1; }
port="$2"
mkdir -p target/ota
python3 tools/check-artifacts.py bootloader
espflash save-image --skip-update-check --chip esp32 --flash-size 4mb --flash-mode dio --flash-freq 40mhz "$elf" target/ota/nekodoa.bin
tools/install-ota.sh "$port"
exec espflash monitor --skip-update-check --chip esp32 --port "$port" --monitor-baud 115200 --log-format serial
