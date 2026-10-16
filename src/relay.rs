use core::cell::RefCell;
use critical_section::Mutex;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::Timer;
use esp_hal::gpio::{Level, Output};
struct State {
    pin: Option<Output<'static>>,
    gate: crate::relay_gate::Gate,
}
static RELAY: Mutex<RefCell<State>> = Mutex::new(RefCell::new(State {
    pin: None,
    gate: crate::relay_gate::Gate::new(),
}));
static PULSE: Signal<CriticalSectionRawMutex, u16> = Signal::new();
pub fn init(output: Output<'static>) {
    critical_section::with(|cs| RELAY.borrow_ref_mut(cs).pin = Some(output));
}
pub fn set(high: bool) -> bool {
    let changed = critical_section::with(|cs| {
        let mut state = RELAY.borrow_ref_mut(cs);
        if state.gate.updating {
            return false;
        }
        state
            .pin
            .as_mut()
            .expect("Relay initialized")
            .set_level(if high { Level::High } else { Level::Low });
        state.gate.high = high;
        true
    });
    if changed {
        log::info!("Door relay GPIO26: {}", if high { "HIGH" } else { "LOW" });
    }
    changed
}
pub fn request_pulse(seconds: u16) {
    critical_section::with(|cs| {
        let mut state = RELAY.borrow_ref_mut(cs);
        if state.gate.request_pulse() {
            PULSE.signal(seconds);
        }
    });
}
pub fn updating() -> bool {
    critical_section::with(|cs| RELAY.borrow_ref(cs).gate.updating)
}
pub fn begin_update() -> bool {
    critical_section::with(|cs| {
        let mut state = RELAY.borrow_ref_mut(cs);
        if state.pin.is_none() || !state.gate.begin_update() {
            return false;
        }
        state.pin.as_mut().unwrap().set_high();
        true
    })
}
pub fn end_update() {
    critical_section::with(|cs| {
        let mut state = RELAY.borrow_ref_mut(cs);
        PULSE.reset();
        state.gate.end_update();
    });
}
#[embassy_executor::task]
pub async fn pulse_task() {
    loop {
        let seconds = PULSE.wait().await;
        let accepted = critical_section::with(|cs| {
            let mut state = RELAY.borrow_ref_mut(cs);
            if !state.gate.start_pulse() {
                return false;
            }
            state.pin.as_mut().unwrap().set_low();
            true
        });
        if !accepted {
            continue;
        }
        Timer::after_secs(u64::from(seconds)).await;
        critical_section::with(|cs| {
            let mut state = RELAY.borrow_ref_mut(cs);
            state.pin.as_mut().unwrap().set_high();
            state.gate.high = true;
            state.gate.pulsing = false;
        });
    }
}
