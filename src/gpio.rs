//! Read-only digital GPIO snapshot for the original ESP32.
use alloc::string::String;
use core::fmt::Write;

pub fn snapshot() -> String {
    // SAFETY: These fixed ESP32 register blocks are valid after HAL initialization.
    // Only volatile, non-clearing register reads are used. No peripherals are
    // stolen and no pin mux, pull resistor, direction or output is changed.
    let (gpio, mux, rtc) = unsafe {
        (
            &*esp32::GPIO::ptr(),
            &*esp32::IO_MUX::ptr(),
            &*esp32::RTC_IO::ptr(),
        )
    };
    let inputs = u64::from(gpio.in_().read().bits()) | (u64::from(gpio.in1().read().bits()) << 32);
    let outputs =
        u64::from(gpio.out().read().bits()) | (u64::from(gpio.out1().read().bits()) << 32);
    let enabled =
        u64::from(gpio.enable().read().bits()) | (u64::from(gpio.enable1().read().bits()) << 32);
    let mut json = String::with_capacity(5000);
    write!(
        json,
        "{{\"uptime\":{},\"pins\":[",
        embassy_time::Instant::now().as_secs()
    )
    .unwrap();
    for pin in 0..40 {
        if pin != 0 {
            json.push(',');
        }
        let note = match pin {
            20 | 24 | 28..=31 => "Not present on ESP32",
            6..=11 => "Reserved for flash",
            37 | 38 => "Not exposed on WROOM-32",
            1 => "UART0 TX / serial log",
            3 => "UART0 RX",
            16 => "RFID UART2 RX / 9600 8N2",
            17 => "RFID UART2 TX / 9600 8N2",
            26 => "Door relay; default HIGH",
            27 => "DHT temperature / humidity",
            0 | 2 | 5 | 12 | 15 => "Boot strap pin",
            34..=39 => "Input only; no internal pulls",
            _ => "",
        };
        let mut level = "null";
        let mut latch = "null";
        let mode;
        if matches!(pin, 20 | 24 | 28..=31 | 37 | 38) {
            mode = "Unavailable";
        } else if (6..=11).contains(&pin) {
            mode = "Flash";
        } else {
            let bits = mux_bits(mux, pin);
            let input_enabled = bits & (1 << 9) != 0;
            let function = (bits >> 12) & 7;
            let route = gpio.func_out_sel_cfg(pin).read();
            let bit = 1u64 << pin;
            if rtc_selected(rtc, pin) {
                mode = "RTC / analog";
            } else if bits & (1 << 1) != 0 {
                mode = "Sleep configuration";
            } else {
                if input_enabled {
                    level = if inputs & bit != 0 { "true" } else { "false" };
                }
                if function != 2 || route.out_sel().bits() != 256 {
                    mode = "Peripheral";
                } else if enabled & bit != 0 && pin < 34 {
                    mode = if input_enabled {
                        "Input + output"
                    } else {
                        "Output"
                    };
                    // This is the software output latch, not a pad measurement.
                    latch = if outputs & bit != 0 { "true" } else { "false" };
                } else {
                    mode = if input_enabled {
                        "Input"
                    } else {
                        "Input disabled"
                    };
                }
            }
        }
        write!(json, "{{\"pin\":{pin},\"level\":{level},\"latch\":{latch},\"mode\":\"{mode}\",\"note\":\"{note}\"}}").unwrap();
    }
    json.push(']');
    crate::dht22::append_json(&mut json);
    crate::rfid::append_json(&mut json);
    crate::logs::append_json(&mut json);
    write!(
        json,
        ",\"ota\":{{\"ready\":{},\"updating\":{},\"slot\":{}}}",
        crate::ota::ready(),
        crate::relay::updating(),
        crate::flash::booted_slot().map_or(-1, |slot| slot as i32)
    )
    .unwrap();
    json.push('}');
    json
}

fn mux_bits(mux: &esp32::io_mux::RegisterBlock, pin: usize) -> u32 {
    match pin {
        0 => mux.gpio0().read().bits(),
        1 => mux.gpio1().read().bits(),
        2 => mux.gpio2().read().bits(),
        3 => mux.gpio3().read().bits(),
        4 => mux.gpio4().read().bits(),
        5 => mux.gpio5().read().bits(),
        6 => mux.gpio6().read().bits(),
        7 => mux.gpio7().read().bits(),
        8 => mux.gpio8().read().bits(),
        9 => mux.gpio9().read().bits(),
        10 => mux.gpio10().read().bits(),
        11 => mux.gpio11().read().bits(),
        12 => mux.gpio12().read().bits(),
        13 => mux.gpio13().read().bits(),
        14 => mux.gpio14().read().bits(),
        15 => mux.gpio15().read().bits(),
        16 => mux.gpio16().read().bits(),
        17 => mux.gpio17().read().bits(),
        18 => mux.gpio18().read().bits(),
        19 => mux.gpio19().read().bits(),
        21 => mux.gpio21().read().bits(),
        22 => mux.gpio22().read().bits(),
        23 => mux.gpio23().read().bits(),
        25 => mux.gpio25().read().bits(),
        26 => mux.gpio26().read().bits(),
        27 => mux.gpio27().read().bits(),
        32 => mux.gpio32().read().bits(),
        33 => mux.gpio33().read().bits(),
        34 => mux.gpio34().read().bits(),
        35 => mux.gpio35().read().bits(),
        36 => mux.gpio36().read().bits(),
        37 => mux.gpio37().read().bits(),
        38 => mux.gpio38().read().bits(),
        39 => mux.gpio39().read().bits(),
        _ => unreachable!(),
    }
}

fn rtc_selected(rtc: &esp32::rtc_io::RegisterBlock, pin: usize) -> bool {
    match pin {
        0 => rtc.touch_pad1().read().mux_sel().bit(),
        2 => rtc.touch_pad2().read().mux_sel().bit(),
        4 => rtc.touch_pad0().read().mux_sel().bit(),
        12 => rtc.touch_pad5().read().mux_sel().bit(),
        13 => rtc.touch_pad4().read().mux_sel().bit(),
        14 => rtc.touch_pad6().read().mux_sel().bit(),
        15 => rtc.touch_pad3().read().mux_sel().bit(),
        25 => rtc.pad_dac1().read().mux_sel().bit(),
        26 => rtc.pad_dac2().read().mux_sel().bit(),
        27 => rtc.touch_pad7().read().mux_sel().bit(),
        32 => rtc.xtal_32k_pad().read().x32p_mux_sel().bit(),
        33 => rtc.xtal_32k_pad().read().x32n_mux_sel().bit(),
        34 => rtc.adc_pad().read().adc1_mux_sel().bit(),
        35 => rtc.adc_pad().read().adc2_mux_sel().bit(),
        36 => rtc.sensor_pads().read().sense1_mux_sel().bit(),
        39 => rtc.sensor_pads().read().sense4_mux_sel().bit(),
        _ => false,
    }
}
