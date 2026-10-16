#!/usr/bin/env python3
"""Run the actual USB migration script against a simulated flash, never a device."""
import hashlib
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib

root = Path(__file__).resolve().parent.parent
app = (root/'target/ota/nekodoa.bin').read_bytes()
with tempfile.TemporaryDirectory(prefix='nekodoa-install-') as folder:
    temp = Path(folder)
    flash = bytearray(b'\xff' * 0x400000)
    flash[0x10000:0x10000+len(app)] = app
    record = b'preserved-authorization-and-duration'
    record += b'\x5a'*(4096-len(record))
    assert len(record) == 4096
    flash[0x3f0000:0x3f1000] = record
    (temp/'flash.bin').write_bytes(flash)
    (temp/'espflash').write_text('''#!/usr/bin/env python3
import os, sys
from pathlib import Path
args = sys.argv[1:]
assert args[args.index('--port')+1] == 'SIMULATED', 'Never connect a real device'
root = Path(os.environ['OTA_TEST_DIR'])
path = root/'flash.bin'
if args[0] == 'board-info':
    print('Flash size:        _4_MB')
elif args[0] == 'read-flash':
    assert args[args.index('--max-in-flight')+1] == '1', 'Do not pipeline flash read packets'
    assert args[args.index('--baud')+1] == os.environ.get('ESPFLASH_BAUD', '115200')
    if os.environ.get('OTA_TEST_FAIL_READ'): sys.exit(42)
    data = path.read_bytes()
    Path(args[-1]).write_bytes(data[int(args[-3],0):int(args[-3],0)+int(args[-2],0)])
elif args[0] == 'write-bin':
    offset = int(args[-2],0)
    data = Path(args[-1]).read_bytes()
    assert offset in [0x1f0000,0x8000,0xe000,0x1000]
    flash = bytearray(path.read_bytes()); flash[offset:offset+len(data)] = data; path.write_bytes(flash)
    with (root/'writes').open('a') as log: log.write(str(offset)+'\\n')
else: raise AssertionError(args)
''')
    (temp/'espflash').chmod(0o755)
    env = os.environ.copy()
    env.pop('ESPFLASH_BAUD', None)
    env.update(OTA_TEST_DIR=str(temp), PATH=str(temp)+os.pathsep+env['PATH'])
    subprocess.run(['tools/install-ota.sh','SIMULATED'],cwd=root,env=env,check=True,stdout=subprocess.DEVNULL)
    result = (temp/'flash.bin').read_bytes()
    assert result[0x10000:0x10000+len(app)] == app
    assert result[0x3f0000:0x3f1000] == record
    assert result[0x1f0000:0x1f0000+len(app)] == app
    for index,seq in enumerate((1,2)):
        entry = result[0xe000+index*4096:0xe000+index*4096+32]
        assert struct.unpack_from('<I',entry)[0] == seq
        assert struct.unpack_from('<I',entry,24)[0] == (2 if index==0 else 0)
        assert struct.unpack_from('<I',entry,28)[0] == zlib.crc32(entry[:4],0xffffffff)
    assert (temp/'writes').read_text().splitlines() == [str(i) for i in (0x1f0000,0x8000,0xe000,0x1000)]
    before = (temp/'flash.bin').read_bytes()
    writes = (temp/'writes').read_text()
    env.update(OTA_TEST_FAIL_READ='1', ESPFLASH_BAUD='460800')
    failed = subprocess.run(['tools/install-ota.sh','SIMULATED'],cwd=root,env=env,capture_output=True,text=True)
    assert failed.returncode == 42
    assert 'Backup failed. Installation has not written flash.' in failed.stderr
    assert (temp/'flash.bin').read_bytes() == before
    assert (temp/'writes').read_text() == writes
    # Remove simulated backups only, never backups from a real board.
    for backup in (root/'target/ota').glob('backup-*.bin'):
        if hashlib.sha256(backup.read_bytes()).digest() == hashlib.sha256(flash).digest():
            backup.unlink()
print('USB installation simulation passed: previous app, settings, boot sequence, baud override and no writes after backup failure.')
