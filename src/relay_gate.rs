//! Relay/update decisions run under the same critical-section lock as GPIO writes.
#[derive(Default)]
pub struct Gate {
    pub high: bool,
    pub pending: bool,
    pub pulsing: bool,
    pub updating: bool,
}

impl Gate {
    pub const fn new() -> Self {
        Self {
            high: true,
            pending: false,
            pulsing: false,
            updating: false,
        }
    }
    pub fn begin_update(&mut self) -> bool {
        if self.updating || !self.high || self.pending || self.pulsing {
            return false;
        }
        self.updating = true;
        true
    }
    pub fn request_pulse(&mut self) -> bool {
        if self.updating {
            return false;
        }
        self.pending = true;
        true
    }
    pub fn start_pulse(&mut self) -> bool {
        self.pending = false;
        if self.updating {
            return false;
        }
        self.pulsing = true;
        self.high = false;
        true
    }
    pub fn end_update(&mut self) {
        self.pending = false;
        self.updating = false;
    }
}
