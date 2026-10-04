use core::cell::RefCell;
use critical_section::Mutex;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use esp_hal::gpio::{Level, Output};

static RELAY: Mutex<RefCell<Option<Output<'static>>>> = Mutex::new(RefCell::new(None));
static PULSE: Signal<CriticalSectionRawMutex, u16> = Signal::new();

pub fn init(output: Output<'static>) {
    critical_section::with(|cs| RELAY.borrow_ref_mut(cs).replace(output));
}

// Both HTTP workers share one owned pin. No critical section spans an await.
pub fn set(high: bool) {
    critical_section::with(|cs| {
        RELAY
            .borrow_ref_mut(cs)
            .as_mut()
            .expect("Relay initialized before HTTP")
            .set_level(if high { Level::High } else { Level::Low });
    });
    log::info!("Door relay GPIO26: {}", if high { "HIGH" } else { "LOW" });
}

pub fn request_pulse(seconds: u16) {
    PULSE.signal(seconds);
}

#[embassy_executor::task]
pub async fn pulse_task() {
    loop {
        let seconds = PULSE.wait().await;
        set(false);
        Timer::after_secs(u64::from(seconds)).await;
        set(true);
    }
}
