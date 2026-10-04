//! Small persistent authorization record in a reserved flash sector.
//! The default 4 MiB ESP32 flash layout leaves this sector outside the app.

#[cfg(not(test))]
const ADDRESS: u32 = 0x3F0000;
#[cfg(not(test))]
use core::cell::RefCell;
#[cfg(not(test))]
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
#[cfg(not(test))]
static FLASH: critical_section::Mutex<RefCell<Option<esp_storage::FlashStorage<'static>>>> =
    critical_section::Mutex::new(RefCell::new(None));

#[cfg(not(test))]
pub fn init(flash: esp_hal::peripherals::FLASH<'static>) {
    let storage = esp_storage::FlashStorage::new(flash);
    critical_section::with(|cs| *FLASH.borrow(cs).borrow_mut() = Some(storage));
}
const MAGIC: &[u8; 4] = b"NKA2";
pub const DEFAULT_RELAY_SECONDS: u16 = 10;
pub const MAX_IDS: usize = 8;
pub const ID_LEN: usize = 20;
const RECORD_LEN: usize = 4 + 2 + 1 + MAX_IDS * (1 + ID_LEN) + 1;

// Keep both the length and the buffer address word-aligned for ROM flash I/O.
#[repr(align(4))]
struct Record([u8; RECORD_LEN.next_multiple_of(4)]);

#[cfg(not(test))]
pub fn load(ids: &mut [[u8; ID_LEN]; MAX_IDS], lengths: &mut [u8; MAX_IDS]) -> (usize, u16, bool) {
    let mut data = Record([0; RECORD_LEN.next_multiple_of(4)]);
    let read = critical_section::with(|cs| {
        let mut flash = FLASH.borrow(cs).borrow_mut();
        flash
            .as_mut()
            .is_some_and(|flash| flash.read(ADDRESS, &mut data.0).is_ok())
    });
    if !read {
        return (0, DEFAULT_RELAY_SECONDS, false);
    }
    decode(&data.0, ids, lengths)
}

fn decode(
    data: &[u8],
    ids: &mut [[u8; ID_LEN]; MAX_IDS],
    lengths: &mut [u8; MAX_IDS],
) -> (usize, u16, bool) {
    if data.len() < RECORD_LEN || &data[..4] != MAGIC || data[6] as usize > MAX_IDS {
        return (0, DEFAULT_RELAY_SECONDS, false);
    }
    let mut checksum = 0u8;
    for byte in &data[..RECORD_LEN - 1] {
        checksum ^= *byte;
    }
    if checksum != data[RECORD_LEN - 1] {
        return (0, DEFAULT_RELAY_SECONDS, false);
    }
    let seconds = u16::from_le_bytes([data[4], data[5]]);
    if !(1..=300).contains(&seconds) {
        return (0, DEFAULT_RELAY_SECONDS, false);
    }
    let count = data[6] as usize;
    let mut offset = 7;
    for i in 0..count {
        let len = data[offset] as usize;
        if len == 0
            || len > ID_LEN
            || !data[offset + 1..offset + 1 + len]
                .iter()
                .all(u8::is_ascii_digit)
        {
            return (0, DEFAULT_RELAY_SECONDS, false);
        }
        ids[i][..len].copy_from_slice(&data[offset + 1..offset + 1 + len]);
        lengths[i] = len as u8;
        offset += 1 + ID_LEN;
    }
    (count, seconds, true)
}

fn encode(
    ids: &[[u8; ID_LEN]; MAX_IDS],
    lengths: &[u8; MAX_IDS],
    count: usize,
    seconds: u16,
) -> Option<Record> {
    if count > MAX_IDS
        || lengths[..count]
            .iter()
            .any(|&len| len == 0 || usize::from(len) > ID_LEN)
    {
        return None;
    }
    if !(1..=300).contains(&seconds) {
        return None;
    }
    let mut record = Record([0xFFu8; RECORD_LEN.next_multiple_of(4)]);
    let data = &mut record.0;
    data[..4].copy_from_slice(MAGIC);
    data[4..6].copy_from_slice(&seconds.to_le_bytes());
    data[6] = count as u8;
    let mut offset = 7;
    for i in 0..count {
        let len = usize::from(lengths[i]);
        data[offset] = lengths[i];
        data[offset + 1..offset + 1 + len].copy_from_slice(&ids[i][..len]);
        offset += 1 + ID_LEN;
    }
    let mut checksum = 0u8;
    for byte in &data[..RECORD_LEN - 1] {
        checksum ^= *byte;
    }
    data[RECORD_LEN - 1] = checksum;
    Some(record)
}

#[cfg(not(test))]
pub fn save(
    ids: &[[u8; ID_LEN]; MAX_IDS],
    lengths: &[u8; MAX_IDS],
    count: usize,
    seconds: u16,
) -> bool {
    let Some(data) = encode(ids, lengths, count, seconds) else {
        return false;
    };
    critical_section::with(|cs| {
        let mut flash = FLASH.borrow(cs).borrow_mut();
        let Some(flash) = flash.as_mut() else {
            return false;
        };
        flash.erase(ADDRESS, ADDRESS + 4096).is_ok() && flash.write(ADDRESS, &data.0).is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_id_survives_reload_with_every_valid_duration() {
        let mut ids = [[0; ID_LEN]; MAX_IDS];
        let mut lengths = [0; MAX_IDS];
        for (i, id) in [b"250269590436101", b"900255004170212"].iter().enumerate() {
            ids[i][..id.len()].copy_from_slice(*id);
            lengths[i] = id.len() as u8;
        }
        for seconds in 1..=300 {
            let record = encode(&ids, &lengths, 2, seconds).unwrap();
            let mut loaded = [[0; ID_LEN]; MAX_IDS];
            let mut loaded_lengths = [0; MAX_IDS];
            assert_eq!(
                decode(&record.0, &mut loaded, &mut loaded_lengths),
                (2, seconds, true)
            );
            assert_eq!(loaded, ids);
            assert_eq!(loaded_lengths, lengths);
        }
    }

    #[test]
    fn corrupt_counts_never_panic_even_with_valid_checksum() {
        for count in 9..=255 {
            let mut record = encode(&[[0; ID_LEN]; MAX_IDS], &[0; MAX_IDS], 0, 10).unwrap();
            record.0[6] = count;
            record.0[RECORD_LEN - 1] = record.0[..RECORD_LEN - 1].iter().fold(0, |a, b| a ^ b);
            assert_eq!(
                decode(&record.0, &mut [[0; ID_LEN]; MAX_IDS], &mut [0; MAX_IDS]),
                (0, 10, false)
            );
        }
    }

    #[test]
    fn damaged_and_erased_records_are_rejected() {
        let mut record = encode(&[[0; ID_LEN]; MAX_IDS], &[0; MAX_IDS], 0, 10).unwrap();
        record.0[8] ^= 1;
        for data in [&record.0[..], &[0xFF; RECORD_LEN], &[]] {
            assert!(!decode(data, &mut [[0; ID_LEN]; MAX_IDS], &mut [0; MAX_IDS]).2);
        }
    }
}
