#[cfg(test)]
#[path = "../../../src/http.rs"]
mod http;
#[cfg(test)]
#[path = "../../../src/auth_storage.rs"]
mod auth_storage;
#[cfg(test)]
#[path = "../../../src/ota_format.rs"]
mod ota_format;

#[cfg(test)]
mod tests {
    use super::{http::*, ota_format::*};
    use sha2::{Digest, Sha256};
    fn image() -> Vec<u8> {
        let mut data = vec![0; 24];
        data[4..8].copy_from_slice(&0x40080000u32.to_le_bytes());
        data[0] = 0xe9; data[1] = 1; data[23] = 1;
        data.extend_from_slice(&0x3f400020u32.to_le_bytes());
        data.extend_from_slice(&256u32.to_le_bytes());
        data.extend_from_slice(&0xabcd5432u32.to_le_bytes());
        data.resize(288, 0);
        let checksum = data[32..].iter().fold(0xef, |c,b| c ^ b);
        data.resize(304,0); data[303] = checksum;
        let digest = Sha256::digest(&data); data.extend_from_slice(&digest);
        data
    }
    fn valid(data: &[u8]) -> bool { validate(data.len(), |offset,out| { if offset+out.len()>data.len() { return false; } out.copy_from_slice(&data[offset..offset+out.len()]); true }) }
    #[test]
    fn binary_body_and_every_fragment_boundary() {
        let mut request = b"POST /api/ota HTTP/1.1\r\nContent-Length: 336\r\nX-OTA-Control: 1\r\n\r\n".to_vec();
        let end = request.len(); request.extend_from_slice(&image());
        for split in 0..request.len() {
            let mut fragments = request[..split].to_vec();
            if split < end { assert_eq!(header_end(&fragments),None); }
            fragments.extend_from_slice(&request[split..]);
            assert_eq!(header_end(&fragments),Some(end));
            assert_eq!(route(&fragments),Route::Ota);
            assert_eq!(ota_length(&fragments),Some(336));
            assert!(valid(&fragments[end..]));
        }
    }
    #[test]
    fn reject_lengths_and_cross_origin_forms() {
        assert_eq!(relay_seconds(b"POST /api/relay-duration?seconds=10 HTTP/1.1\r\n\r\n"), Some(10));
        for headers in ["", "Content-Length: 0\r\n", "Content-Length: 1966081\r\n", "Content-Length: -336\r\n", "Content-Length: +336\r\n", "Content-Length: 336\r\nContent-Length: 336\r\n", "Content-Length: 336\r\nTransfer-Encoding: chunked\r\n", "Content-Length: 9999999999999999999999999999999\r\n"] {
            let request = format!("POST /api/ota HTTP/1.1\r\nX-OTA-Control: 1\r\n{headers}\r\n");
            assert_eq!(ota_length(request.as_bytes()),None);
        }
        assert_eq!(route(b"POST /api/ota HTTP/1.1\r\n\r\n"),Route::Forbidden);
        assert_eq!(route(b"OPTIONS /api/ota HTTP/1.1\r\n\r\n"),Route::MethodNotAllowed);
    }
    #[test]
    fn rejects_wrong_chip_merged_truncated_corrupt_and_trailing_images() {
        let good = image(); assert!(valid(&good));
        for index in [0,1,12,23,28,32,40,303,335] {
            let mut bad = good.clone(); bad[index] ^= 0x40; assert!(!valid(&bad),"offset {index}");
        }
        for length in 0..good.len() { assert!(!valid(&good[..length])); }
        let mut merged = vec![0xff;0x10000]; merged.extend_from_slice(&good); assert!(!valid(&merged));
        let mut extra = good.clone(); extra.push(0xff); assert!(!valid(&extra));
        assert!(!validate(good.len(), |_,_| false));
    }
    #[test]
    fn crc_matches_idf_and_ignores_aborted_or_corrupt_selection() {
        // Python zlib.crc32(pack('<I', 1), 0xffffffff) / ESP ROM CRC32_LE.
        assert_eq!(crc(&1u32.to_le_bytes()),0x4743989a);
        for state in [0,1,2,u32::MAX] { assert_eq!(selection(&entry(1,state)),Some((1,0,state))); }
        assert_eq!(selection(&entry(2,2)),Some((2,1,2)));
        for state in [3,4,5] { assert!(selection(&entry(3,state)).is_none()); }
        for seq in [0,u32::MAX] { assert!(selection(&entry(seq,2)).is_none()); }
        let mut bad = entry(7,2); bad[28] ^= 1; assert!(selection(&bad).is_none());
    }
    #[test]
    fn reject_small_or_incompatible_flash_layout() {
        let mut table = vec![];
        for (kind,subtype,address,size) in [(1,2,0x9000,0x5000),(1,0,OTA_DATA,0x2000),(0,0x10,SLOTS[0],SLOT_SIZE as u32),(0,0x11,SLOTS[1],SLOT_SIZE as u32),(1,0x40,0x3f0000,0x1000)] {
            let mut p = [0u8;32]; p[..4].copy_from_slice(&[0xaa,0x50,kind,subtype]); p[4..8].copy_from_slice(&address.to_le_bytes()); p[8..12].copy_from_slice(&size.to_le_bytes()); table.extend_from_slice(&p);
        }
        assert!(layout(&table,0x400000)); assert!(!layout(&table,0x200000));
        table[100] ^= 1; assert!(!layout(&table,0x400000));
    }
    #[test]
    fn exported_firmware_passes_same_device_validator() {
        if let Ok(data) = std::fs::read("../../target/ota/nekodoa.bin") { assert!(valid(&data)); }
    }
}

#[cfg(test)]
#[path = "../../../src/relay_gate.rs"]
mod relay_gate;
#[cfg(test)]
mod safety_tests {
    use super::{ota_format::*, relay_gate::Gate};
    #[test]
    fn relay_update_and_pending_pulses_are_mutually_exclusive() {
        let mut gate = Gate::new();
        assert!(gate.begin_update());
        assert!(!gate.begin_update(), "concurrent upload rejected");
        assert!(!gate.request_pulse());
        assert!(!gate.start_pulse());
        assert!(gate.high);
        gate.end_update(); assert!(!gate.pending);
        assert!(gate.request_pulse()); assert!(!gate.begin_update());
        assert!(gate.start_pulse()); assert!(!gate.begin_update());
        gate.pulsing = false; assert!(!gate.begin_update(), "manual LOW rejected");
        gate.high = true; assert!(gate.begin_update());
    }
    #[test]
    fn interrupted_or_invalid_upload_never_activates_or_erases_running_slot() {
        use std::cell::RefCell;
        let data = RefCell::new(vec![0xff;0x400000]);
        data.borrow_mut()[OTA_DATA as usize..OTA_DATA as usize+32].copy_from_slice(&entry(1,2));
        let commit = |_,_,_| panic!("activation must not occur");
        let read = |address: u32, out: &mut [u8]| { out.copy_from_slice(&data.borrow()[address as usize..address as usize+out.len()]); true };
        let mut upload = UploadedImage { slot: 1, length: 2048, written: 0 };
        assert!(!upload.activate(read, commit));
        assert!(upload.chunk(&mut [0;1024],1024, |address| { assert_eq!(address,SLOTS[1]); true }, |address,bytes| { data.borrow_mut()[address as usize..address as usize+bytes.len()].copy_from_slice(bytes); true }));
        assert!(!upload.activate(read, commit));
        assert!(upload.chunk(&mut [0;1024],1024, |_| panic!("no redundant sector erase"), |address,bytes| { data.borrow_mut()[address as usize..address as usize+bytes.len()].copy_from_slice(bytes); true }));
        assert!(!upload.activate(read, commit));
        assert!(data.borrow()[SLOTS[0] as usize..SLOTS[0] as usize+SLOT_SIZE].iter().all(|&b| b==0xff));
        assert_eq!(&data.borrow()[OTA_DATA as usize..OTA_DATA as usize+32],&entry(1,2));
    }
    #[test]
    fn valid_upload_activates_both_directions_and_preserves_fallback() {
        use std::cell::RefCell;
        let exported = std::fs::read("../../target/ota/nekodoa.bin").expect("Run tools/export-ota.sh before tests");
        for current_slot in 0..=1 {
            let data = RefCell::new(vec![0xff;0x400000]);
            let old = entry(current_slot as u32+1,2);
            data.borrow_mut()[OTA_DATA as usize..OTA_DATA as usize+32].copy_from_slice(&old);
            // An aborted entry has a higher sequence; it must not select the active slot.
            let aborted = entry(40,4);
            data.borrow_mut()[OTA_DATA as usize+4096..OTA_DATA as usize+4096+32].copy_from_slice(&aborted);
            let mut upload = UploadedImage { slot:1-current_slot, length:exported.len(), written:0 };
            for chunk in exported.chunks(1024) {
                let mut buffer = [0xff;1024]; buffer[..chunk.len()].copy_from_slice(chunk);
                assert!(upload.chunk(&mut buffer,chunk.len(), |address| { assert!(address>=SLOTS[1-current_slot] && address<SLOTS[1-current_slot]+SLOT_SIZE as u32); data.borrow_mut()[address as usize..address as usize+4096].fill(0xff); true }, |address,bytes| { data.borrow_mut()[address as usize..address as usize+bytes.len()].copy_from_slice(bytes); true }));
            }
            let mut activated = false;
            assert!(upload.activate(|address,out| { out.copy_from_slice(&data.borrow()[address as usize..address as usize+out.len()]); true }, |index,seq,state| { assert_eq!(index,1); assert_eq!(state,0); assert_eq!(((seq-1)%2) as usize,1-current_slot); activated = true; true }));
            assert!(activated);
            assert_eq!(&data.borrow()[OTA_DATA as usize..OTA_DATA as usize+32],&old);
            assert!(data.borrow()[0x3f0000..0x3f1000].iter().all(|&b| b==0xff));
        }
    }
}
