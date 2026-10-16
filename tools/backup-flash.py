#!/usr/bin/env python3
"""Back up flash with visible status while espflash buffers the read in memory."""
import os
import subprocess
import sys
import time

port, destination = sys.argv[1:]
baud = os.environ.get('ESPFLASH_BAUD', '115200')
print(f'Backing up all 4 MiB at {baud} baud to {destination}.', flush=True)
print('This may take several minutes. No flash writes occur until the backup is verified.', flush=True)
command = ['espflash', 'read-flash', '--skip-update-check', '--chip', 'esp32',
           '--port', port, '--baud', baud, '--max-in-flight', '1',
           '--after', 'no-reset', '0', '0x400000', destination]
# espflash 4.5.0 clears buffered serial input before sending each read ACK.
# Its default window of 64 packets can therefore discard pipelined flash data.
# A one-packet window keeps the next block waiting until the previous ACK.
started = time.monotonic()
with subprocess.Popen(command) as process:
    try:
        while True:
            try:
                result = process.wait(timeout=10)
                break
            except subprocess.TimeoutExpired:
                elapsed = int(time.monotonic() - started)
                print(f'Backup in progress ({elapsed}s elapsed); waiting for espflash to finish.', flush=True)
    except KeyboardInterrupt:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()
        print('Backup interrupted. Installation has not written flash.', file=sys.stderr)
        raise SystemExit(130)
if result:
    print('Backup failed. Installation has not written flash.', file=sys.stderr)
    raise SystemExit(result)
print(f'Backup read complete in {int(time.monotonic() - started)}s; validating it next.', flush=True)
