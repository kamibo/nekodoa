#!/usr/bin/env bash
# Initial migration/recovery only. Never erase the whole chip or authorization sector.
set -euo pipefail
cd "$(dirname "$0")/.."
port="${1:?Usage: tools/install-ota.sh /dev/ttyUSB0}"
python3 tools/check-artifacts.py bootloader
python3 tools/check-artifacts.py application target/ota/nekodoa.bin
printf 'Checking device and physical flash size…\n'
espflash board-info --skip-update-check --chip esp32 --port "$port" --after no-reset > target/ota/board-info.txt
python3 tools/check-artifacts.py device target/ota/board-info.txt
# Keep a full USB backup, including the existing app and settings, before any writes.
backup="target/ota/backup-$(date -u +%Y%m%dT%H%M%S%NZ).bin"
python3 tools/backup-flash.py "$port" "$backup"
python3 tools/check-artifacts.py backup "$backup"
# Install OTA firmware into slot 1 so the existing slot-0 app remains intact.
# The new app confirms this entry after 30 healthy seconds. The bootloader can
# fall back to the preserved legacy application if migration startup fails.
printf 'Backup verified. Installing application into slot 1…\n'
espflash write-bin --skip-update-check --chip esp32 --port "$port" --after no-reset 0x1f0000 target/ota/nekodoa.bin
printf 'Installing OTA partition table…\n'
espflash write-bin --skip-update-check --chip esp32 --port "$port" --after no-reset 0x8000 bootloader/build/partition_table/partition-table.bin
printf 'Installing OTA boot selection…\n'
python3 tools/check-artifacts.py initial-metadata
espflash write-bin --skip-update-check --chip esp32 --port "$port" --after no-reset 0xe000 target/ota/initial-otadata.bin
# Bootloader last: the currently installed app and authorization data are retained.
printf 'Installing rollback bootloader and restarting…\n'
espflash write-bin --skip-update-check --chip esp32 --port "$port" 0x1000 bootloader/build/bootloader/bootloader.bin
printf 'USB backup saved to %s\n' "$backup"
