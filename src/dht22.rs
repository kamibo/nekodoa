use crate::dht_frame::{self, Model, Reading};
use alloc::string::String;
use core::{cell::Cell, fmt::Write};
use critical_section::Mutex;
use embassy_time::Timer;
use esp_hal::{
    gpio::{DriveMode, Flex, InputConfig, OutputConfig, Pull},
    peripherals::GPIO27,
    time::Instant,
};

#[derive(Clone, Copy)]
struct State {
    reading: Option<Reading>,
    sampled_at: u64,
    status: &'static str,
}
static STATE: Mutex<Cell<State>> = Mutex::new(Cell::new(State {
    reading: None,
    sampled_at: 0,
    status: "starting",
}));

struct Capture {
    frame: [u8; 5],
    response_us: [u64; 3],
    high_us: [u64; 40],
    bits_received: usize,
}

fn configured_model() -> Model {
    match option_env!("DHT_MODEL") {
        Some("DHT11") => Model::Dht11,
        Some("DHT22") | None => Model::Dht22,
        _ => panic!("DHT_MODEL must be DHT11 or DHT22"),
    }
}

#[embassy_executor::task]
pub async fn sample(pin: GPIO27<'static>) {
    let model = configured_model();
    log::info!("Sensor on GPIO27: {} decoding", model.name());
    let mut pin = Flex::new(pin);
    pin.set_high();
    pin.apply_output_config(&OutputConfig::default().with_drive_mode(DriveMode::OpenDrain));
    pin.apply_input_config(&InputConfig::default().with_pull(Pull::Up));
    pin.set_input_enable(true);
    pin.set_output_enable(true);
    // Sensor power-up settling time; subsequent transactions are >2 s apart.
    Timer::after_secs(2).await;
    let mut previous_status = "starting";
    let mut consecutive_errors = 0u32;
    loop {
        let mut capture = Capture {
            frame: [0; 5],
            response_us: [0; 3],
            high_us: [0; 40],
            bits_received: 0,
        };
        pin.set_low();
        Timer::after_millis(model.start_low_ms()).await;
        // DHT pulses are only 26–70 us. Protect the bounded receive window from
        // preemption; do not hold a critical section during the start pulse/wait.
        let result = critical_section::with(|_| {
            pin.set_high(); // Open drain: release, never drive the sensor HIGH.
            receive(&pin, &mut capture, model)
        });
        pin.set_high();
        let status = result.as_ref().map(|_| "ok").unwrap_or_else(|error| *error);
        let state = State {
            reading: result.ok(),
            sampled_at: embassy_time::Instant::now().as_secs(),
            status,
        };
        critical_section::with(|cs| STATE.borrow(cs).set(state));
        if status == "ok" {
            consecutive_errors = 0;
        } else {
            consecutive_errors = consecutive_errors.saturating_add(1);
            // Capture within the timing window, print only after interrupts are
            // restored. Rate-limit persistent faults to avoid flooding UART.
            if consecutive_errors <= 3 || consecutive_errors % 10 == 0 {
                let raw = dht_frame::unpack_for(capture.frame, model);
                let checksum = capture.frame[..4]
                    .iter()
                    .fold(0u8, |sum, byte| sum.wrapping_add(*byte));
                log::warn!(
                    "{} diagnostic: error={} bits={} frame={:02X?} checksum={:02X}/{:02X} raw_temp={:.1}C raw_humidity={:.1}% (unvalidated)",
                    model.name(),
                    status,
                    capture.bits_received,
                    capture.frame,
                    checksum,
                    capture.frame[4],
                    f32::from(raw.temperature) / 10.0,
                    f32::from(raw.humidity) / 10.0,
                );
                log::warn!(
                    "{} timing us: release/response={:?} data_high={:?}",
                    model.name(),
                    capture.response_us,
                    &capture.high_us[..capture.bits_received]
                );
            }
        }
        if status != previous_status {
            if status == "ok" {
                log::info!("{} on GPIO27 responding", model.name());
            } else {
                log::warn!("{} on GPIO27: {}", model.name(), status);
            }
            previous_status = status;
        }
        Timer::after_secs(3).await;
    }
}

fn receive(pin: &Flex<'_>, capture: &mut Capture, model: Model) -> Result<Reading, &'static str> {
    let transaction_start = Instant::now();
    let wait = |high: bool| -> Result<u64, &'static str> {
        let start = Instant::now();
        while pin.is_high() == high {
            if start.elapsed().as_micros() > 120 || transaction_start.elapsed().as_micros() > 6000 {
                return Err("timeout / check wiring");
            }
        }
        Ok(start.elapsed().as_micros())
    };
    capture.response_us[0] = wait(true)?; // Host release -> sensor response LOW.
    capture.response_us[1] = wait(false)?; // Sensor response LOW, approximately 80 us.
    capture.response_us[2] = wait(true)?; // Sensor response HIGH, approximately 80 us.
    for bit in 0..40 {
        wait(false)?;
        let duration = wait(true)?;
        capture.high_us[bit] = duration;
        capture.frame[bit / 8] = (capture.frame[bit / 8] << 1) | u8::from(duration > 50);
        capture.bits_received = bit + 1;
    }
    dht_frame::decode_for(capture.frame, model)
}

pub fn append_json(json: &mut String) {
    let state = critical_section::with(|cs| STATE.borrow(cs).get());
    let age = embassy_time::Instant::now()
        .as_secs()
        .saturating_sub(state.sampled_at);
    let status = if state.reading.is_some() && age > 10 {
        "stale"
    } else {
        state.status
    };
    write!(
        json,
        ",\"sensor\":{{\"pin\":27,\"model\":\"{}\",\"status\":\"{status}\",\"age_seconds\":{age},",
        configured_model().name()
    )
    .unwrap();
    if let Some(reading) = state.reading.filter(|_| age <= 10) {
        write!(
            json,
            "\"temperature_c\":{:.1},\"humidity_percent\":{:.1}}}",
            f32::from(reading.temperature) / 10.0,
            f32::from(reading.humidity) / 10.0
        )
        .unwrap();
    } else {
        json.push_str("\"temperature_c\":null,\"humidity_percent\":null}");
    }
}
