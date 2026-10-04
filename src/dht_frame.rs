//! Explicit DHT11/DHT22 payload decoding; both use a five-byte checksum frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Model {
    Dht11,
    Dht22,
}

impl Model {
    pub fn name(self) -> &'static str {
        match self {
            Self::Dht11 => "DHT11",
            Self::Dht22 => "DHT22",
        }
    }
    pub fn start_low_ms(self) -> u64 {
        match self {
            Self::Dht11 => 20,
            Self::Dht22 => 2,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    pub temperature: i16,
    pub humidity: u16,
}

pub fn decode_for(bytes: [u8; 5], model: Model) -> Result<Reading, &'static str> {
    let checksum = bytes[..4]
        .iter()
        .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    if checksum != bytes[4] {
        return Err("checksum error");
    }
    if model == Model::Dht11 && (bytes[1] > 9 || bytes[3] & 0x7f > 9) {
        return Err("invalid DHT11 decimal byte");
    }
    let reading = unpack_for(bytes, model);
    if reading.humidity > 1000 || !(-400..=800).contains(&reading.temperature) {
        return Err("out of range");
    }
    Ok(reading)
}

/// Interpret the payload for diagnostics only; no checksum/range validation.
pub fn unpack_for(bytes: [u8; 5], model: Model) -> Reading {
    if model == Model::Dht11 {
        let integer = i16::from(bytes[2]);
        let fraction = i16::from(bytes[3] & 0x7f);
        return Reading {
            humidity: u16::from(bytes[0]) * 10 + u16::from(bytes[1]),
            // DHT11 negative values use the complemented integer part.
            temperature: if bytes[3] & 0x80 != 0 {
                -(integer + 1) * 10 + fraction
            } else {
                integer * 10 + fraction
            },
        };
    }
    let humidity = u16::from_be_bytes([bytes[0], bytes[1]]);
    let raw = u16::from_be_bytes([bytes[2], bytes[3]]);
    let magnitude = (raw & 0x7fff) as i16;
    let temperature = if raw & 0x8000 != 0 {
        -magnitude
    } else {
        magnitude
    };
    Reading {
        temperature,
        humidity,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn decode(bytes: [u8; 5]) -> Result<Reading, &'static str> {
        decode_for(bytes, Model::Dht22)
    }
    fn unpack(bytes: [u8; 5]) -> Reading {
        unpack_for(bytes, Model::Dht22)
    }
    fn frame(humidity: u16, temperature: u16) -> [u8; 5] {
        let [a, b] = humidity.to_be_bytes();
        let [c, d] = temperature.to_be_bytes();
        [
            a,
            b,
            c,
            d,
            a.wrapping_add(b).wrapping_add(c).wrapping_add(d),
        ]
    }
    #[test]
    fn decodes_positive_negative_and_zero() {
        assert_eq!(
            decode(frame(658, 263)),
            Ok(Reading {
                humidity: 658,
                temperature: 263
            })
        );
        assert_eq!(
            decode(frame(450, 0x807b)),
            Ok(Reading {
                humidity: 450,
                temperature: -123
            })
        );
        assert_eq!(
            decode(frame(0, 0)),
            Ok(Reading {
                humidity: 0,
                temperature: 0
            })
        );
        assert!(decode(frame(1000, 800)).is_ok());
        assert!(decode(frame(1000, 0x8190)).is_ok());
    }
    #[test]
    fn rejects_corruption_and_invalid_ranges() {
        let mut bad = frame(658, 263);
        bad[4] ^= 1;
        assert_eq!(decode(bad), Err("checksum error"));
        assert_eq!(decode(frame(1001, 200)), Err("out of range"));
        assert_eq!(decode(frame(500, 801)), Err("out of range"));
        assert_eq!(decode(frame(500, 0x8191)), Err("out of range"));
    }

    #[test]
    fn diagnostics_preserve_rejected_payload() {
        let bytes = [55, 0, 24, 0, 79];
        assert_eq!(decode(bytes), Err("out of range"));
        assert_eq!(
            unpack(bytes),
            Reading {
                humidity: 14080,
                temperature: 6144
            }
        );
    }

    #[test]
    fn decodes_actual_arceli_frames_as_dht11() {
        for (bytes, temperature) in [
            ([0x2f, 0, 0x1d, 1, 0x4d], 291),
            ([0x2f, 0, 0x1d, 0, 0x4c], 290),
        ] {
            assert_eq!(
                decode_for(bytes, Model::Dht11),
                Ok(Reading {
                    humidity: 470,
                    temperature
                })
            );
            assert_eq!(decode_for(bytes, Model::Dht22), Err("out of range"));
        }
        assert_eq!(Model::Dht11.start_low_ms(), 20);
        assert_eq!(Model::Dht22.start_low_ms(), 2);
        assert_eq!(Model::Dht11.name(), "DHT11");
    }

    #[test]
    fn dht11_rejects_corruption_and_invalid_decimal_bytes() {
        assert_eq!(
            decode_for([47, 0, 29, 1, 0], Model::Dht11),
            Err("checksum error")
        );
        assert_eq!(
            decode_for([47, 10, 29, 0, 86], Model::Dht11),
            Err("invalid DHT11 decimal byte")
        );
        assert_eq!(
            decode_for([101, 0, 29, 0, 130], Model::Dht11),
            Err("out of range")
        );
        assert_eq!(
            decode_for([47, 0, 12, 0x87, 194], Model::Dht11),
            Ok(Reading {
                humidity: 470,
                temperature: -123
            })
        );
    }
}
