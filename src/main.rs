#![no_std]
#![no_main]

extern crate alloc;

use embassy_executor::Spawner;
use esp_backtrace as _;
use esp_hal::{
    clock::CpuClock,
    gpio::{Level, Output, OutputConfig},
    interrupt::software::SoftwareInterruptControl,
    timer::timg::TimerGroup,
    uart::{Config as UartConfig, UartTx},
};

mod auth_storage;
mod dht22;
mod dht_frame;
mod flash;
mod gpio;
mod http;
mod logs;
mod ota;
mod ota_format;
mod relay;
mod relay_gate;
mod rfid;
mod rfid_capture;
mod rfid_frame;
mod web;
mod wifi;

// Stable metadata makes application exports reproducible; version identifies builds.
esp_bootloader_esp_idf::esp_app_desc!(
    env!("CARGO_PKG_VERSION"),
    env!("CARGO_PKG_NAME"),
    "00:00:00",
    "2025-09-23",
    "5.5.1",
    esp_bootloader_esp_idf::MMU_PAGE_SIZE,
    0,
    u16::MAX,
    esp_bootloader_esp_idf::SECURE_VERSION
);

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    // Arm before settings, UART and network startup. A stalled pending image resets.
    let mut watchdog = TimerGroup::new(peripherals.TIMG1).wdt;
    watchdog.set_timeout(
        esp_hal::timer::timg::MwdtStage::Stage0,
        esp_hal::time::Duration::from_secs(60),
    );
    watchdog.enable();
    flash::init(peripherals.FLASH);
    // Set HIGH before enabling the output, then give the controller ownership.
    relay::init(Output::new(
        peripherals.GPIO26,
        Level::High,
        OutputConfig::default(),
    ));
    if ota::startup_pending() {
        assert!(relay::begin_update());
    } else {
        watchdog.disable();
    }
    spawner.spawn(relay::pulse_task().expect("Failed to spawn relay pulse task"));
    // The first HAL UART init resets shared UART memory. Do it before logging,
    // then restore UART0's FIFO and baud configuration for esp-println's ROM
    // writer. Keep both handles alive so their clock guards remain held.
    let (rfid_rx, rfid_tx) = rfid::init(peripherals.UART2, peripherals.GPIO16, peripherals.GPIO17);
    let _console = UartTx::new(
        peripherals.UART0,
        UartConfig::default().with_baudrate(115_200),
    )
    .expect("Invalid console UART configuration")
    .with_tx(peripherals.GPIO1);
    // esp-println is the only writer; do not write through _console as well.
    logs::init();
    log::info!("Door relay configured on GPIO26: HIGH");

    // Wi-Fi driver and scheduler allocations, following the esp-radio DHCP example.
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 36 * 1024);

    let timer_group = TimerGroup::new(peripherals.TIMG0);
    let software_interrupt = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timer_group.timer0, software_interrupt.software_interrupt0);
    spawner.spawn(dht22::sample(peripherals.GPIO27).expect("Failed to spawn DHT22 task"));
    spawner.spawn(rfid::receive(rfid_rx).expect("Failed to spawn RFID task"));
    spawner.spawn(rfid::transmit(rfid_tx).expect("Failed to spawn RFID TX task"));

    let stack = match option_env!("WIFI_PASSWORD") {
        Some(password) => Some(wifi::start(peripherals.WIFI, spawner, password)),
        None => {
            log::warn!("Wi-Fi not configured: set WIFI_PASSWORD at build time (see README.md)");
            None
        }
    };

    if stack.is_none() && !ota::startup_pending() {
        watchdog.disable();
    }
    if let Some(stack) = stack {
        spawner.spawn(ota::verify(stack, watchdog).expect("Failed to spawn OTA verifier"));
        for _ in 0..2 {
            spawner.spawn(web::serve(stack).expect("Failed to spawn web server"));
        }
    }

    if let Some(stack) = stack {
        loop {
            stack.wait_config_up().await;
            if let Some(config) = stack.config_v4() {
                log::info!("GPIO dashboard: http://{}/", config.address.address());
            }
            stack.wait_config_down().await;
            log::warn!("Wi-Fi IP address lost; waiting for DHCP");
        }
    }

    core::future::pending::<()>().await;
    unreachable!()
}
