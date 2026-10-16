#!/usr/bin/env python3
"""Validate the actual IDF artifacts and ESP32 application exported by espflash."""
import csv
import hashlib
import pathlib
import re
import struct
import sys
import zlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SIZE = 0x1E0000
EXPECTED = [(1,2,0x9000,0x5000),(1,0,0xE000,0x2000),(0,0x10,0x10000,SIZE),(0,0x11,0x1F0000,SIZE),(1,0x40,0x3F0000,0x1000)]

def table_check():
    lines = [line for line in (ROOT/'partitions.csv').read_text().splitlines() if not line.startswith('#')]
    rows = list(csv.reader(lines))
    assert len(rows) == 5
    end = 0x9000
    for row, (_,_,address,size) in zip(rows,EXPECTED):
        assert int(row[3].strip(),0) == address and int(row[4].strip(),0) == size
        assert address >= end and address % (0x10000 if row[1].strip() == 'app' else 4096) == 0
        end = address + size
    assert end <= 0x400000

def application(path):
    image = pathlib.Path(path).read_bytes()
    assert 288 <= len(image) <= SIZE, 'Application size outside slot limits'
    assert image[0] == 0xE9 and 1 <= image[1] <= 16 and image[12:14] == b'\0\0' and image[23] in (0,1), 'Not an ESP32 app image'
    offset, checksum = 24, 0xEF
    for segment in range(image[1]):
        _, size = struct.unpack_from('<II', image, offset)
        offset += 8
        assert size and size % 4 == 0 and offset + size <= len(image)
        if segment == 0:
            assert size >= 256 and image[offset:offset+4] == bytes.fromhex('3254cdab'), 'Application descriptor missing'
        for byte in image[offset:offset+size]:
            checksum ^= byte
        offset += size
    padded = (offset + 16) & ~15
    assert all(b == 0 for b in image[offset:padded-1]) and image[padded-1] == checksum, 'Checksum failure'
    assert len(image) == padded + (32 if image[23] else 0), 'Trailing data / merged image'
    if image[23]:
        assert hashlib.sha256(image[:padded]).digest() == image[padded:], 'SHA256 failure'
    print(f'Application verified: {len(image)} / {SIZE} bytes; SHA256 {hashlib.sha256(image).hexdigest()}')

def bootloader():
    build = ROOT/'bootloader/build'
    config = (ROOT/'bootloader/sdkconfig').read_text()
    assert 'CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE=y' in config
    assert 'CONFIG_BOOTLOADER_APP_ANTI_ROLLBACK=y' not in config
    binary = (build/'bootloader/bootloader.bin').read_bytes()
    assert binary[0] == 0xE9 and len(binary) <= 0x7000, 'Bootloader overlaps partition table'
    table = (build/'partition_table/partition-table.bin').read_bytes()
    for index, (kind,subtype,address,size) in enumerate(EXPECTED):
        magic,k,s,a,n = struct.unpack_from('<HBBII', table, index*32)
        assert (magic,k,s,a,n) == (0x50AA,kind,subtype,address,size)
    assert table[160:162] == b'\xeb\xeb', 'Unexpected additional partition'
    assert hashlib.md5(table[:160]).digest() == table[176:192]
    print(f'Rollback bootloader verified: {len(binary)} bytes; SHA256 {hashlib.sha256(binary).hexdigest()}')

def initial_metadata():
    # Keep the existing slot-0 app as VALID; boot the migration image once as NEW.
    entries = bytearray(b'\xff'*8192)
    for index,seq in enumerate((1,2)):
        record = struct.pack('<I20sII',seq,b'\xff'*20,2 if index == 0 else 0,zlib.crc32(struct.pack('<I',seq),0xFFFFFFFF))
        entries[index*4096:index*4096+32] = record
    (ROOT/'target/ota/initial-otadata.bin').write_bytes(entries)

table_check()
if sys.argv[1] == 'application': application(sys.argv[2])
elif sys.argv[1] == 'bootloader': bootloader()
elif sys.argv[1] == 'initial-metadata': initial_metadata()
elif sys.argv[1] == 'device':
    text = pathlib.Path(sys.argv[2]).read_text()
    match = re.search(r'Flash size:\s*_?(\d+)[_\s]*M(?:I)?B\b', text, re.IGNORECASE)
    assert match and int(match[1]) >= 4, 'OTA installation requires at least 4 MiB physical flash'
elif sys.argv[1] == 'backup':
    data = pathlib.Path(sys.argv[2]).read_bytes()
    assert len(data) == 0x400000
    # Require the existing application to fit the retained slot before any writes.
    start = 0x10000
    assert data[start] == 0xE9, 'No existing application at 0x10000; use fresh-board installation instructions'
    offset = start + 24
    for _ in range(data[start+1]):
        _,size = struct.unpack_from('<II',data,offset)
        offset += 8 + size
        assert offset <= start + SIZE, 'Existing application overlaps the new inactive slot'
    length = ((offset-start+16)&~15) + (32 if data[start+23] else 0)
    temp = ROOT/'target/ota/previous-app.bin'
    temp.write_bytes(data[start:start+length])
    application(temp)
else: raise SystemExit('Unknown command')
