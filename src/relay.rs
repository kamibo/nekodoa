use core::cell::RefCell;
use critical_section::Mutex;
use esp_hal::gpio::{Level, Output};

static RELAY: Mutex<RefCell<Option<Output<'static>>>> = Mutex::new(RefCell::new(None));

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
