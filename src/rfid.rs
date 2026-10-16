//! WL-134 receive and persistent tag authorization.
use crate::auth_storage::{self, ID_LEN, MAX_IDS};
use crate::rfid_capture::Capture;
use crate::rfid_frame::{DATA_LEN, Parser};
use alloc::string::String;
use core::{cell::Cell, fmt::Write};
use critical_section::Mutex;
use embassy_time::{Duration, Timer, with_timeout};
use esp_hal::{
    Blocking,
    peripherals::{GPIO16, GPIO17, UART2},
    uart::{Config, DataBits, Parity, RxConfig, RxError, StopBits, Uart, UartRx, UartTx},
};

const MAX_AUTHORIZED: usize = MAX_IDS;

#[derive(Clone, Copy)]
struct State {
    last: Capture,
    captured_at: u64,
    captures: u64,
    errors: u64,
    glitches: u64,
    boundary: &'static str,
    status: &'static str,
    code: Option<[u8; DATA_LEN]>,
    code_at: u64,
    codes: u64,
    code_id: [u8; ID_LEN],
    code_id_len: u8,
    authorized: bool,
    ids: [[u8; ID_LEN]; MAX_AUTHORIZED],
    id_lens: [u8; MAX_AUTHORIZED],
    id_count: u8,
    relay_seconds: u16,
}

static STATE: Mutex<Cell<State>> = Mutex::new(Cell::new(State {
    last: Capture::new(),
    captured_at: 0,
    captures: 0,
    errors: 0,
    glitches: 0,
    boundary: "none",
    status: "starting",
    code: None,
    code_at: 0,
    codes: 0,
    code_id: [0; ID_LEN],
    code_id_len: 0,
    authorized: false,
    ids: [
        *b"250269590436101\0\0\0\0\0",
        [0; ID_LEN],
        [0; ID_LEN],
        [0; ID_LEN],
        [0; ID_LEN],
        [0; ID_LEN],
        [0; ID_LEN],
        [0; ID_LEN],
    ],
    id_lens: [15, 0, 0, 0, 0, 0, 0, 0],
    id_count: 1,
    relay_seconds: auth_storage::DEFAULT_RELAY_SECONDS,
}));

fn tag_id(code: &[u8; DATA_LEN]) -> Option<String> {
    fn parse_reversed_hex(bytes: &[u8]) -> Option<u64> {
        let mut value = 0u64;
        for byte in bytes.iter().rev() {
            value = value.checked_mul(16)?.checked_add(match byte {
                b'0'..=b'9' => u64::from(byte - b'0'),
                b'A'..=b'F' => u64::from(byte - b'A' + 10),
                b'a'..=b'f' => u64::from(byte - b'a' + 10),
                _ => return None,
            })?;
        }
        Some(value)
    }
    let animal = parse_reversed_hex(&code[..10])?;
    let country = parse_reversed_hex(&code[10..14])?;
    let mut id = String::new();
    write!(
        id,
        "{}",
        country
            .checked_mul(1_000_000_000_000)?
            .checked_add(animal)?
    )
    .ok()?;
    Some(id)
}

fn publish_code(code: Option<[u8; DATA_LEN]>) {
    let Some(code) = code else {
        return;
    };
    let id = tag_id(&code);
    let (authorized, relay_seconds) = critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).get();
        state.code = Some(code);
        state.code_at = embassy_time::Instant::now().as_secs();
        state.codes = state.codes.saturating_add(1);
        state.code_id = [0; ID_LEN];
        state.code_id_len = 0;
        state.authorized = false;
        if let Some(ref id) = id {
            let bytes = id.as_bytes();
            state.code_id[..bytes.len()].copy_from_slice(bytes);
            state.code_id_len = bytes.len() as u8;
            state.authorized = (0..usize::from(state.id_count)).any(|i| {
                usize::from(state.id_lens[i]) == bytes.len()
                    && state.ids[i][..bytes.len()] == *bytes
            });
        }
        let result = (state.authorized, state.relay_seconds);
        STATE.borrow(cs).set(state);
        result
    });
    log::info!(
        "RFID candidate code (unverified): {}",
        core::str::from_utf8(&code).unwrap()
    );
    if authorized {
        log::info!(
            "RFID authorized tag {}: opening door for {} seconds",
            id.as_deref().unwrap_or("unknown"),
            relay_seconds
        );
        crate::relay::request_pulse(relay_seconds);
    } else {
        log::warn!(
            "RFID unauthorized tag {}",
            id.as_deref().unwrap_or("unknown")
        );
    }
}

pub fn add_authorized(id: &[u8]) -> bool {
    if crate::relay::updating() {
        return false;
    }
    if id.is_empty() || id.len() > ID_LEN || !id.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let changed = critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).get();
        if (0..usize::from(state.id_count))
            .any(|i| state.id_lens[i] as usize == id.len() && state.ids[i][..id.len()] == *id)
        {
            return true;
        }
        let index = usize::from(state.id_count);
        if index >= MAX_AUTHORIZED {
            return false;
        }
        state.ids[index][..id.len()].copy_from_slice(id);
        state.id_lens[index] = id.len() as u8;
        state.id_count += 1;
        STATE.borrow(cs).set(state);
        true
    });
    if changed {
        persist_authorized();
    }
    changed
}

pub fn delete_authorized(id: &[u8]) -> bool {
    if crate::relay::updating() {
        return false;
    }
    let changed = critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).get();
        let Some(index) = (0..usize::from(state.id_count))
            .find(|&i| state.id_lens[i] as usize == id.len() && state.ids[i][..id.len()] == *id)
        else {
            return false;
        };
        if index + 1 < usize::from(state.id_count) {
            state.ids[index] = state.ids[usize::from(state.id_count) - 1];
            state.id_lens[index] = state.id_lens[usize::from(state.id_count) - 1];
        }
        state.id_count -= 1;
        STATE.borrow(cs).set(state);
        true
    });
    if changed {
        persist_authorized();
    }
    changed
}

fn persist_authorized() {
    let (ids, lengths, count, relay_seconds) = critical_section::with(|cs| {
        let state = STATE.borrow(cs).get();
        (
            state.ids,
            state.id_lens,
            usize::from(state.id_count),
            state.relay_seconds,
        )
    });
    if !auth_storage::save(&ids, &lengths, count, relay_seconds) {
        log::error!("Could not persist RFID authorization list");
    }
}

fn publish(capture: &mut Capture, boundary: &'static str) {
    if capture.is_empty() {
        return;
    }
    let last = *capture;
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).get();
        state.last = last;
        state.captured_at = embassy_time::Instant::now().as_secs();
        state.captures = state.captures.saturating_add(1);
        state.boundary = boundary;
        state.status = "received";
        STATE.borrow(cs).set(state);
    });
    log::info!(
        "RFID raw (unvalidated, {} bytes, {}): {}",
        last.bytes().len(),
        boundary,
        last.hex()
    );
    *capture = Capture::new();
}

/// Initialize before the console/logger: the first HAL UART initialization
/// resets shared UART memory on ESP32, including the ROM console's FIFO.
pub fn init(
    uart: UART2<'static>,
    rx_pin: GPIO16<'static>,
    tx_pin: GPIO17<'static>,
) -> (UartRx<'static, Blocking>, UartTx<'static, Blocking>) {
    let mut ids = [[0; ID_LEN]; MAX_AUTHORIZED];
    let mut lengths = [0; MAX_AUTHORIZED];
    let (count, relay_seconds, valid) = auth_storage::load(&mut ids, &mut lengths);
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).get();
        state.relay_seconds = relay_seconds;
        if valid {
            state.ids = ids;
            state.id_lens = lengths;
            state.id_count = count as u8;
        }
        STATE.borrow(cs).set(state);
    });
    let config = Config::default()
        .with_baudrate(9600)
        .with_data_bits(DataBits::_8)
        .with_parity(Parity::None)
        .with_stop_bits(StopBits::_2)
        .with_rx(RxConfig::default().with_fifo_full_threshold(32));
    Uart::new(uart, config)
        .expect("Invalid RFID UART configuration")
        .with_rx(rx_pin)
        .with_tx(tx_pin)
        .split()
}

pub fn set_relay_seconds(seconds: u16) -> bool {
    if crate::relay::updating() {
        return false;
    }
    if !(1..=300).contains(&seconds) {
        return false;
    }
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).get();
        state.relay_seconds = seconds;
        STATE.borrow(cs).set(state);
    });
    persist_authorized();
    true
}

#[embassy_executor::task]
pub async fn receive(rx: UartRx<'static, Blocking>) {
    // Only enable the async interrupt handler once the scheduler is running.
    let mut rx = rx.into_async();
    critical_section::with(|cs| {
        let mut state = STATE.borrow(cs).get();
        state.status = "listening";
        STATE.borrow(cs).set(state);
    });
    log::info!("RFID listening on GPIO16 / UART2: 9600 baud, 8N2 WL-134 format");
    let mut capture = Capture::new();
    let mut parser = Parser::default();
    let mut pending = false;
    let mut buffer = [0u8; 32];
    loop {
        // read_async is cancellation safe. An idle gap is a diagnostic grouping
        // heuristic, NOT proof of a complete RFID protocol frame.
        let result = if !pending {
            rx.read_async(&mut buffer).await
        } else {
            match with_timeout(Duration::from_millis(50), rx.read_async(&mut buffer)).await {
                Ok(result) => result,
                Err(_) => {
                    publish_code(parser.idle());
                    publish(&mut capture, "idle gap");
                    pending = false;
                    continue;
                }
            }
        };
        match result {
            Ok(count) => {
                pending |= count != 0;
                log::info!(
                    "RFID UART buffer read: {} bytes: {:02X?}",
                    count,
                    &buffer[..count]
                );
                for &byte in &buffer[..count] {
                    publish_code(parser.feed(byte));
                    if capture.push(byte) {
                        publish(&mut capture, "128-byte limit; may be partial");
                    }
                }
            }
            Err(RxError::GlitchOccurred) => {
                capture = Capture::new();
                parser.reset();
                pending = false;
                critical_section::with(|cs| {
                    let mut state = STATE.borrow(cs).get();
                    state.glitches = state.glitches.saturating_add(1);
                    state.status = "line glitch";
                    STATE.borrow(cs).set(state);
                });
            }
            Err(error) => {
                // Discard a partial capture after a UART error; never combine
                // bytes from before and after a known receive failure.
                capture = Capture::new();
                parser.reset();
                pending = false;
                let errors = critical_section::with(|cs| {
                    let mut state = STATE.borrow(cs).get();
                    state.errors = state.errors.saturating_add(1);
                    state.status = "uart error";
                    STATE.borrow(cs).set(state);
                    state.errors
                });
                if errors <= 3 || errors % 10 == 0 {
                    log::warn!("RFID UART error: {:?}; check wiring and 9600 8N2", error);
                }
                Timer::after_millis(50).await;
            }
        }
    }
}

#[embassy_executor::task]
pub async fn transmit(mut tx: UartTx<'static, Blocking>) {
    log::info!(
        "RFID transmit probe on GPIO17: sending 00 every second (WL-134 normally has no RX)"
    );
    loop {
        if let Err(error) = tx.write(&[0x00]) {
            log::warn!("RFID UART transmit error: {:?}", error);
        }
        Timer::after_secs(1).await;
    }
}

pub fn append_json(json: &mut String) {
    let state = critical_section::with(|cs| STATE.borrow(cs).get());
    write!(json, ",\"rfid\":{{\"pin\":16,\"baud\":9600,\"status\":\"{}\",\"captures\":{},\"uart_errors\":{},\"glitches\":{},\"boundary\":\"{}\",\"age_seconds\":",
        state.status, state.captures, state.errors, state.glitches, state.boundary).unwrap();
    if state.last.is_empty() {
        json.push_str("null,\"hex\":null,\"ascii\":null");
    } else {
        let age = embassy_time::Instant::now()
            .as_secs()
            .saturating_sub(state.captured_at);
        write!(json, "{age},\"hex\":\"{}\",\"ascii\":", state.last.hex()).unwrap();
        state.last.append_ascii_json(json);
    }
    write!(json, ",\"codes\":{},\"code\":", state.codes).unwrap();
    if let Some(code) = state.code {
        let age = embassy_time::Instant::now()
            .as_secs()
            .saturating_sub(state.code_at);
        write!(
            json,
            "\"{}\",\"code_age_seconds\":{age}",
            core::str::from_utf8(&code).unwrap()
        )
        .unwrap();
    } else {
        json.push_str("null,\"code_age_seconds\":null");
    }
    write!(
        json,
        ",\"relay_seconds\":{},\"tag_id\":{},\"authorized\":{},\"authorized_ids\":[",
        state.relay_seconds,
        if state.code_id_len == 0 {
            "null"
        } else {
            core::str::from_utf8(&state.code_id[..usize::from(state.code_id_len)]).unwrap()
        },
        state.authorized
    )
    .unwrap();
    for i in 0..usize::from(state.id_count) {
        if i != 0 {
            json.push(',');
        }
        write!(
            json,
            "\"{}\"",
            core::str::from_utf8(&state.ids[i][..usize::from(state.id_lens[i])]).unwrap()
        )
        .unwrap();
    }
    json.push_str("]}");
}
