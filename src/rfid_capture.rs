//! Bounded diagnostic captures, not a tag/protocol decoder.
extern crate alloc;

use alloc::string::String;
use core::fmt::Write;

pub const CAPACITY: usize = 128;

#[derive(Clone, Copy)]
pub struct Capture {
    bytes: [u8; CAPACITY],
    len: usize,
}

impl Capture {
    pub const fn new() -> Self {
        Self {
            bytes: [0; CAPACITY],
            len: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Caller must publish/reset as soon as this returns true.
    pub fn push(&mut self, byte: u8) -> bool {
        self.bytes[self.len] = byte;
        self.len += 1;
        self.len == CAPACITY
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }

    pub fn hex(&self) -> String {
        let mut text = String::with_capacity(self.len * 3);
        for (index, byte) in self.bytes().iter().enumerate() {
            if index != 0 {
                text.push(' ');
            }
            write!(text, "{byte:02X}").unwrap();
        }
        text
    }

    /// Printable ASCII only; dots represent all control/non-ASCII bytes.
    /// JSON metacharacters are escaped; callers must use textContent in HTML.
    pub fn append_ascii_json(&self, json: &mut String) {
        json.push('"');
        for &byte in self.bytes() {
            match byte {
                b'"' => json.push_str("\\\""),
                b'\\' => json.push_str("\\\\"),
                0x20..=0x7e => json.push(char::from(byte)),
                _ => json.push('.'),
            }
        }
        json.push('"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_and_json_escaping() {
        let mut capture = Capture::new();
        assert!(capture.is_empty());
        for byte in [0x02, b'A', b'"', b'\\', 0x0d, 0xff, 0x03] {
            assert!(!capture.push(byte));
        }
        assert_eq!(capture.hex(), "02 41 22 5C 0D FF 03");
        let mut json = String::new();
        capture.append_ascii_json(&mut json);
        assert_eq!(json, "\".A\\\"\\\\...\"");
    }

    #[test]
    fn full_capture_and_remainder_preserve_all_bytes() {
        let mut capture = Capture::new();
        let mut collected = alloc::vec::Vec::new();
        for byte in (0..=255).chain([42]) {
            if capture.push(byte) {
                collected.extend_from_slice(capture.bytes());
                capture = Capture::new();
            }
        }
        assert_eq!(capture.bytes(), &[42]);
        collected.extend_from_slice(capture.bytes());
        assert_eq!(
            collected,
            (0..=255).chain([42]).collect::<alloc::vec::Vec<u8>>()
        );
    }
}
