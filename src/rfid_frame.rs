//! WL-134 FDX-B frame parser: STX, 26 ASCII hex bytes, two checksum bytes, ETX.
pub const DATA_LEN: usize = 26;

#[derive(Default)]
pub struct Parser {
    data: [u8; DATA_LEN],
    len: usize,
    checksum: [u8; 2],
    checksum_len: usize,
    started: bool,
    invalid: bool,
}

impl Parser {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    pub fn feed(&mut self, byte: u8) -> Option<[u8; DATA_LEN]> {
        if byte == 0x02 {
            self.reset();
            self.started = true;
            return None;
        }
        if !self.started {
            return None;
        }
        if self.len < DATA_LEN {
            if byte.is_ascii_hexdigit() {
                self.data[self.len] = byte.to_ascii_uppercase();
                self.len += 1;
            } else {
                self.invalid = true;
            }
            return None;
        }
        if self.checksum_len < 2 {
            self.checksum[self.checksum_len] = byte;
            self.checksum_len += 1;
            return None;
        }
        let expected = self.data.iter().fold(0u8, |sum, byte| sum ^ byte);
        let result = if byte == 0x03 && !self.invalid && self.checksum == [expected, !expected] {
            Some(self.data)
        } else {
            None
        };
        self.reset();
        result
    }
    pub fn idle(&mut self) -> Option<[u8; DATA_LEN]> {
        self.reset();
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &[u8] = b"\x024E7DE6F5B34830110000000000\x7C\x83\x03";
    #[test]
    fn parses_wl134_sample() {
        let mut parser = Parser::default();
        let mut result = None;
        for byte in SAMPLE {
            result = parser.feed(*byte).or(result);
        }
        assert_eq!(result, Some(*b"4E7DE6F5B34830110000000000"));
    }
    #[test]
    fn rejects_bad_frames() {
        for sample in [
            b"\x024E7DE6F5B3483011000000000\x7C\x83\x03".as_slice(),
            b"\x024E7DE6F5B34830110000000000\x7D\x82\x03".as_slice(),
            b"\x024E7DE6F5B3483011000000000G\x7C\x83\x03".as_slice(),
        ] {
            let mut parser = Parser::default();
            assert!(
                sample
                    .iter()
                    .filter_map(|b| parser.feed(*b))
                    .next()
                    .is_none()
            );
        }
    }
}
