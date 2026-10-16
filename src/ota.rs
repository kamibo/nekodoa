use crate::{flash, ota_format as format};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Instant, Timer};
static SETTINGS: AtomicBool = AtomicBool::new(false);
static READY: AtomicBool = AtomicBool::new(false);
static HTTP: AtomicBool = AtomicBool::new(false);
pub fn settings_loaded(read: bool) {
    SETTINGS.store(read, Ordering::Relaxed);
}
pub fn http_ready() {
    HTTP.store(true, Ordering::Relaxed);
}
#[repr(align(4))]
pub struct Buffer(pub [u8; 1024]);
#[repr(align(4))]
struct Entry([u8; 32]);
fn layout_ok() -> bool {
    let mut table = [0; 0xc00];
    flash::read(0x8000, &mut table) && format::layout(&table, flash::capacity())
}
fn entries() -> Option<[[u8; 32]; 2]> {
    let mut result = [[0; 32]; 2];
    for (i, entry) in result.iter_mut().enumerate() {
        if !flash::read(format::OTA_DATA + i as u32 * 4096, entry) {
            return None;
        }
    }
    Some(result)
}
fn write_entry(index: usize, sequence: u32, state: u32) -> bool {
    let data = Entry(format::entry(sequence, state));
    let address = format::OTA_DATA + index as u32 * 4096;
    let mut check = [0; 32];
    flash::erase_sector(address)
        && flash::write(address, &data.0)
        && flash::read(address, &mut check)
        && check == data.0
}

/// RAII restores relay operation on disconnect, timeout, validation error or cancellation.
pub struct Upload {
    image: format::UploadedImage,
    activated: bool,
}
impl Upload {
    pub fn begin(length: usize) -> Result<Self, &'static str> {
        if !(288..=format::SLOT_SIZE).contains(&length) {
            return Err("Invalid application size");
        }
        if !READY.load(Ordering::Relaxed) {
            return Err("Startup verification incomplete or incompatible flash layout");
        }
        if !crate::relay::begin_update() {
            return Err("Update busy, relay active or pulse pending");
        }
        let Some(slot) = flash::booted_slot() else {
            crate::relay::end_update();
            return Err("Cannot identify running application");
        };
        Ok(Self {
            image: format::UploadedImage {
                slot: 1 - slot,
                length,
                written: 0,
            },
            activated: false,
        })
    }
    /// Word-aligned chunks except the final chunk, padded with erased bytes.
    pub async fn chunk(&mut self, buffer: &mut Buffer, size: usize) -> Result<(), &'static str> {
        if !self
            .image
            .chunk(&mut buffer.0, size, flash::erase_sector, flash::write)
        {
            return Err("Invalid upload chunk or flash operation failed");
        }
        Timer::after_millis(1).await;
        Ok(())
    }
    pub fn activate(&mut self, started: Instant) -> Result<(), &'static str> {
        if !self.image.activate(flash::read, |index, sequence, state| {
            if started.elapsed().as_secs() >= 300 {
                return false;
            }
            // A flash failure during metadata commit can leave a valid NEW entry.
            // Reset with the door locked if commit cannot be read back reliably.
            self.activated = true;
            write_entry(index, sequence, state)
        }) {
            if self.activated {
                esp_hal::system::software_reset();
            }
            return Err("Invalid ESP32 application, checksum or OTA metadata; activation failed");
        }
        self.activated = true;
        Ok(())
    }
}
impl Drop for Upload {
    fn drop(&mut self) {
        if !self.activated {
            crate::relay::end_update();
        }
    }
}

/// The hardware watchdog starts in main before settings and task initialization.
#[embassy_executor::task]
pub async fn verify(
    stack: embassy_net::Stack<'static>,
    mut watchdog: esp_hal::timer::timg::Wdt<esp_hal::peripherals::TIMG1<'static>>,
) {
    let mut healthy_since = None;
    loop {
        let healthy = SETTINGS.load(Ordering::Relaxed)
            && HTTP.load(Ordering::Relaxed)
            && stack.is_config_up()
            && stack.is_link_up();
        if healthy {
            let since = *healthy_since.get_or_insert(Instant::now());
            if since.elapsed().as_secs() >= 30 {
                // Do not restart the 60-second deadline while startup is unverified.
                if layout_ok() {
                    let slot = flash::booted_slot().expect("Booted OTA partition");
                    let records = entries().expect("Read OTA metadata");
                    let confirmed = if let Some((index, sequence, state)) =
                        format::running_entry(slot, &records)
                    {
                        matches!(state, 1 | 2) && (state == 2 || write_entry(index, sequence, 2))
                    } else if records.iter().all(|e| e.iter().all(|&b| b == 0xff)) {
                        // Initial USB boot has erased metadata: register a known-good fallback.
                        write_entry(0, slot as u32 + 1, 2)
                    } else {
                        false
                    };
                    if !confirmed {
                        esp_hal::system::software_reset();
                    }
                    READY.store(true, Ordering::Relaxed);
                } else if startup_pending() {
                    esp_hal::system::software_reset();
                }
                crate::relay::end_update();
                watchdog.disable();
                log::info!(
                    "Startup healthy; OTA ready: {}",
                    READY.load(Ordering::Relaxed)
                );
                return;
            }
        } else {
            healthy_since = None;
        }
        Timer::after_millis(250).await;
    }
}

pub fn ready() -> bool {
    READY.load(Ordering::Relaxed)
}
pub fn startup_pending() -> bool {
    flash::booted_slot()
        .and_then(|slot| entries().and_then(|e| format::running_entry(slot, &e)))
        .is_some_and(|e| matches!(e.2, 0 | 1))
}
