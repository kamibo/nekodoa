//! Relay/update decisions run under the same critical-section lock as GPIO writes.
#[derive(Default)]
pub struct Gate {
    pub high: bool,
    pub pending: bool,
    pub pulsing: bool,
    pub updating: bool,
    /// RFID-triggered HIGH-to-LOW transitions since boot.
    pub unlocks: u64,
}

impl Gate {
    pub const fn new() -> Self {
        Self {
            high: true,
            pending: false,
            pulsing: false,
            updating: false,
            unlocks: 0,
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
        if self.high {
            self.unlocks = self.unlocks.saturating_add(1);
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

#[cfg(test)]
mod tests {
    use super::Gate;

    #[test]
    fn unlocks_count_activations_instead_of_reports() {
        let mut gate = Gate::new();
        assert_eq!(gate.unlocks, 0);
        for _ in 0..10 {
            assert!(gate.request_pulse());
        }
        assert_eq!(
            gate.unlocks, 0,
            "queued reports have not activated the relay"
        );
        assert!(gate.start_pulse());
        assert_eq!(gate.unlocks, 1);
        for _ in 0..10 {
            assert!(gate.request_pulse());
        }
        assert_eq!(
            gate.unlocks, 1,
            "reports during a pulse do not count separately"
        );
        gate.high = true;
        gate.pulsing = false;
        assert!(gate.start_pulse());
        assert_eq!(gate.unlocks, 2, "a subsequent activation counts once");
    }

    #[test]
    fn blocked_and_already_low_pulses_do_not_count_as_unlocks() {
        let mut gate = Gate::new();
        assert!(gate.begin_update());
        assert!(!gate.request_pulse());
        assert!(!gate.start_pulse());
        assert_eq!(gate.unlocks, 0);
        gate.end_update();
        gate.high = false; // Manually activated relay.
        assert!(gate.request_pulse());
        assert!(gate.start_pulse());
        assert_eq!(gate.unlocks, 0);
        gate.high = true;
        gate.pulsing = false;
        gate.unlocks = u64::MAX;
        assert!(gate.request_pulse());
        assert!(gate.start_pulse());
        assert_eq!(gate.unlocks, u64::MAX, "counter must not wrap");
    }
}
