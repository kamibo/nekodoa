#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
export SOURCE_DATE_EPOCH=1758585600
# Use an activated ESP-IDF v5.5.1 environment, or its versioned official image.
if [[ -n "${IDF_PATH:-}" ]]; then
  [[ "$(git -C "$IDF_PATH" describe --tags --exact-match)" == v5.5.1 ]]
  idf.py -C bootloader -B "$PWD/bootloader/build" -D SDKCONFIG_DEFAULTS=sdkconfig.defaults bootloader partition-table
else
  docker run --rm -v "$PWD:/project" -w /project -e SOURCE_DATE_EPOCH=1758585600 \
    espressif/idf:v5.5.1 bash -c 'idf.py -C bootloader -B /project/bootloader/build -D SDKCONFIG_DEFAULTS=sdkconfig.defaults bootloader partition-table'
fi
python3 tools/check-artifacts.py bootloader
