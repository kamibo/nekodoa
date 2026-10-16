//! Bounded in-memory log ring shared by the serial logger and web dashboard.
use alloc::string::String;
use core::{cell::RefCell, fmt::Write};
use critical_section::Mutex;

const CAPACITY: usize = 32;
// Leave room for the timestamp without reducing the existing message budget.
const LINE_LEN: usize = 224;

#[derive(Clone, Copy)]
struct Line {
    bytes: [u8; LINE_LEN],
    len: u16,
}

impl Line {
    const EMPTY: Self = Self {
        bytes: [0; LINE_LEN],
        len: 0,
    };
}

struct Ring {
    lines: [Line; CAPACITY],
    next: usize,
    count: usize,
}
static RING: Mutex<RefCell<Ring>> = Mutex::new(RefCell::new(Ring {
    lines: [Line::EMPTY; CAPACITY],
    next: 0,
    count: 0,
}));

struct Buffer {
    bytes: [u8; LINE_LEN],
    len: usize,
}
impl Write for Buffer {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let available = LINE_LEN.saturating_sub(self.len);
        let copied = text.len().min(available);
        self.bytes[self.len..self.len + copied].copy_from_slice(&text.as_bytes()[..copied]);
        self.len += copied;
        Ok(())
    }
}

pub fn init() {
    unsafe {
        log::set_logger_racy(&Logger).unwrap();
        log::set_max_level_racy(log::LevelFilter::Info);
    }
}

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let mut line = Buffer {
            bytes: [0; LINE_LEN],
            len: 0,
        };
        // HAL time is available for startup logs before the async timer is started.
        let millis = esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_millis();
        let seconds = millis / 1000;
        let _ = write!(
            &mut line,
            "{} [{:02}:{:02}:{:02}.{:03}] - {}",
            record.level(),
            seconds / 3600,
            (seconds / 60) % 60,
            seconds % 60,
            millis % 1000,
            record.args(),
        );
        let text = core::str::from_utf8(&line.bytes[..line.len]).unwrap_or("invalid log");
        critical_section::with(|cs| {
            let mut ring = RING.borrow_ref_mut(cs);
            let next = ring.next;
            ring.lines[next].bytes[..line.len].copy_from_slice(&line.bytes[..line.len]);
            ring.lines[next].len = line.len as u16;
            ring.next = (next + 1) % CAPACITY;
            ring.count = ring.count.saturating_add(1).min(CAPACITY);
        });
        // Keep the existing serial output behavior.
        esp_println::println!("{}", text);
    }
    fn flush(&self) {}
}

fn json_escape(json: &mut String, bytes: &[u8]) {
    for &byte in bytes {
        match byte {
            b'"' => json.push_str("\\\""),
            b'\\' => json.push_str("\\\\"),
            b'\n' => json.push_str("\\n"),
            b'\r' => json.push_str("\\r"),
            0x20..=0x7e => json.push(byte as char),
            _ => json.push('?'),
        }
    }
}

pub fn append_json(json: &mut String) {
    let (lines, next, count) = critical_section::with(|cs| {
        let ring = RING.borrow_ref(cs);
        (ring.lines, ring.next, ring.count)
    });
    json.push_str(",\"logs\":[");
    let start = (next + CAPACITY - count) % CAPACITY;
    for offset in 0..count {
        if offset != 0 {
            json.push(',');
        }
        json.push('"');
        let line = lines[(start + offset) % CAPACITY];
        json_escape(json, &line.bytes[..usize::from(line.len)]);
        json.push('"');
    }
    json.push(']');
}
