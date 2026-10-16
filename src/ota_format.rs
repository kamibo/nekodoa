//! Pure, bounded ESP32 image and boot-selection validation.
use sha2::{Digest, Sha256};
pub const SLOT_SIZE: usize = 0x1e0000;
pub const SLOTS: [u32; 2] = [0x10000, 0x1f0000];
pub const OTA_DATA: u32 = 0xe000;
pub fn word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().unwrap())
}

/// ESP-IDF uses CRC32 little endian with ROM initial value UINT32_MAX (internally complemented), final XOR.
pub fn crc(bytes: &[u8]) -> u32 {
    let mut result = 0;
    for &byte in bytes {
        result ^= u32::from(byte);
        for _ in 0..8 {
            result = (result >> 1) ^ (0xedb88320 & (0u32.wrapping_sub(result & 1)));
        }
    }
    !result
}
pub fn entry(sequence: u32, state: u32) -> [u8; 32] {
    let mut bytes = [0xff; 32];
    bytes[..4].copy_from_slice(&sequence.to_le_bytes());
    bytes[24..28].copy_from_slice(&state.to_le_bytes());
    bytes[28..32].copy_from_slice(&crc(&sequence.to_le_bytes()).to_le_bytes());
    bytes
}
pub fn selection(bytes: &[u8; 32]) -> Option<(u32, usize, u32)> {
    let seq = word(bytes);
    let state = word(&bytes[24..]);
    (seq != 0
        && seq != u32::MAX
        && word(&bytes[28..]) == crc(&bytes[..4])
        && matches!(state, 0..=2 | u32::MAX))
    .then_some((seq, ((seq.wrapping_sub(1)) % 2) as usize, state))
}

/// Require the exact layout; no extra partition may overlap the reserved ranges.
pub fn layout(table: &[u8], capacity: usize) -> bool {
    if capacity < 0x400000 {
        return false;
    }
    let expected = [
        (1, 2, 0x9000, 0x5000),
        (1, 0, OTA_DATA, 0x2000),
        (0, 0x10, SLOTS[0], SLOT_SIZE as u32),
        (0, 0x11, SLOTS[1], SLOT_SIZE as u32),
        (1, 0x40, 0x3f0000, 0x1000),
    ];
    let mut count = 0;
    for part in table.chunks_exact(32) {
        if part[..2] != [0xaa, 0x50] {
            break;
        }
        if count >= expected.len() {
            return false;
        }
        let (kind, subtype, address, size) = expected[count];
        if part[2] != kind
            || part[3] != subtype
            || word(&part[4..]) != address
            || word(&part[8..]) != size
            || word(&part[28..]) != 0
        {
            return false;
        }
        count += 1;
    }
    count == expected.len()
}

/// Validate application descriptor, chip, segments, XOR checksum and optional SHA256.
/// A reader failure, truncation, trailing bytes or merged flash image is rejected.
pub fn validate(length: usize, mut read: impl FnMut(usize, &mut [u8]) -> bool) -> bool {
    if !(288..=SLOT_SIZE).contains(&length) {
        return false;
    }
    let mut header = [0; 24];
    if !read(0, &mut header)
        || header[0] != 0xe9
        || !(1..=16).contains(&header[1])
        || header[12..14] != [0, 0]
        || header[23] > 1
    {
        return false;
    }
    let entrypoint = word(&header[4..]);
    if header[2] > 3
        || !((0x40080000..0x400c2000).contains(&entrypoint)
            || (0x400d0000..0x40400000).contains(&entrypoint))
    {
        return false;
    }
    let mut hash = Sha256::new();
    hash.update(header);
    let mut checksum = 0xef;
    let mut offset = 24;
    let mut buffer = [0; 1024];
    for segment in 0..header[1] {
        let mut sh = [0; 8];
        if offset + 8 > length || !read(offset, &mut sh) {
            return false;
        }
        hash.update(sh);
        let address = word(&sh) as usize;
        let size = word(&sh[4..]) as usize;
        offset += 8;
        if size == 0 || size % 4 != 0 || size > length - offset {
            return false;
        }
        // ESP32 loadable / mapped application memory ranges (including padding).
        let end = match address.checked_add(size) {
            Some(end) => end,
            None => return false,
        };
        let memory = [
            (0x3f400000, 0x3f800000),
            (0x3ff80000, 0x40000000),
            (0x40080000, 0x400c2000),
            (0x400d0000, 0x40400000),
            (0x50000000, 0x50002000),
        ];
        if address != 0
            && !memory
                .iter()
                .any(|&(low, high)| address >= low && end <= high)
        {
            return false;
        }
        let mapped = (0x3f400000..0x3f800000).contains(&address)
            || (0x400d0000..0x40400000).contains(&address);
        if mapped && address % 65536 != offset % 65536 {
            return false;
        }
        if segment == 0 {
            let mut desc = [0; 4];
            if size < 256 || !read(offset, &mut desc) || word(&desc) != 0xabcd5432 {
                return false;
            }
        }
        let limit = offset + size;
        while offset < limit {
            let n = core::cmp::min(buffer.len(), limit - offset);
            if !read(offset, &mut buffer[..n]) {
                return false;
            }
            hash.update(&buffer[..n]);
            for &byte in &buffer[..n] {
                checksum ^= byte;
            }
            offset += n;
        }
    }
    let padded = (offset + 16) & !15;
    if padded > length || !read(offset, &mut buffer[..padded - offset]) {
        return false;
    }
    if buffer[padded - offset - 1] != checksum
        || buffer[..padded - offset - 1].iter().any(|&b| b != 0)
    {
        return false;
    }
    hash.update(&buffer[..padded - offset]);
    if header[23] == 1 {
        let mut expected = [0; 32];
        padded + 32 == length && read(padded, &mut expected) && hash.finalize()[..] == expected
    } else {
        padded == length
    }
}

pub fn running_entry(slot: usize, entries: &[[u8; 32]; 2]) -> Option<(usize, u32, u32)> {
    entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| selection(e).filter(|e| e.1 == slot).map(|e| (i, e.0, e.2)))
        .max_by_key(|e| e.1)
}
/// All flash mutation is injected, so the exact device path can be tested on a host.
pub struct UploadedImage {
    pub slot: usize,
    pub length: usize,
    pub written: usize,
}
impl UploadedImage {
    pub fn address(&self) -> u32 {
        SLOTS[self.slot]
    }
    pub fn chunk(
        &mut self,
        buffer: &mut [u8; 1024],
        size: usize,
        mut erase: impl FnMut(u32) -> bool,
        mut write: impl FnMut(u32, &[u8]) -> bool,
    ) -> bool {
        if size == 0
            || size > 1024
            || self.written + size > self.length
            || (self.written + size < self.length && size != 1024)
        {
            return false;
        }
        let address = self.address() + self.written as u32;
        if self.written % 4096 == 0 && !erase(address) {
            return false;
        }
        let padded = size.next_multiple_of(4);
        buffer[size..padded].fill(0xff);
        if !write(address, &buffer[..padded]) {
            return false;
        }
        self.written += size;
        true
    }
    pub fn activate(
        &self,
        mut read: impl FnMut(u32, &mut [u8]) -> bool,
        mut commit: impl FnMut(usize, u32, u32) -> bool,
    ) -> bool {
        if self.written != self.length
            || !validate(self.length, |offset, bytes| {
                read(self.address() + offset as u32, bytes)
            })
        {
            return false;
        }
        let mut records = [[0; 32]; 2];
        for (i, record) in records.iter_mut().enumerate() {
            if !read(OTA_DATA + i as u32 * 4096, record) {
                return false;
            }
        }
        let Some((index, _, 2)) = running_entry(1 - self.slot, &records) else {
            return false;
        };
        let max = records
            .iter()
            .filter_map(selection)
            .map(|e| e.0)
            .max()
            .unwrap_or(0);
        let Some(mut next) = max.checked_add(1) else {
            return false;
        };
        if ((next - 1) % 2) as usize != self.slot {
            let Some(value) = next.checked_add(1) else {
                return false;
            };
            next = value;
        }
        next != u32::MAX && commit(1 - index, next, 0)
    }
}
