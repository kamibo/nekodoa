//! One owner for flash, shared by settings and OTA. Every lock is synchronous.
use core::cell::RefCell;
use embedded_storage::{ReadStorage, nor_flash::NorFlash};
static FLASH: critical_section::Mutex<RefCell<Option<esp_storage::FlashStorage<'static>>>> =
    critical_section::Mutex::new(RefCell::new(None));

pub fn init(flash: esp_hal::peripherals::FLASH<'static>) {
    critical_section::with(|cs| {
        *FLASH.borrow_ref_mut(cs) = Some(esp_storage::FlashStorage::new(flash))
    });
}
pub fn read(address: u32, bytes: &mut [u8]) -> bool {
    critical_section::with(|cs| {
        FLASH
            .borrow_ref_mut(cs)
            .as_mut()
            .is_some_and(|f| f.read(address, bytes).is_ok())
    })
}
pub fn capacity() -> usize {
    critical_section::with(|cs| {
        FLASH
            .borrow_ref_mut(cs)
            .as_ref()
            .map_or(0, ReadStorage::capacity)
    })
}
pub fn erase_sector(address: u32) -> bool {
    critical_section::with(|cs| {
        FLASH
            .borrow_ref_mut(cs)
            .as_mut()
            .is_some_and(|f| f.erase(address, address + 4096).is_ok())
    })
}
pub fn write(address: u32, bytes: &[u8]) -> bool {
    critical_section::with(|cs| {
        FLASH
            .borrow_ref_mut(cs)
            .as_mut()
            .is_some_and(|f| NorFlash::write(f, address, bytes).is_ok())
    })
}
pub fn booted_slot() -> Option<usize> {
    use esp_bootloader_esp_idf::partitions::{
        AppPartitionSubType, PartitionType, read_partition_table,
    };
    let mut table = [0; 0xc00];
    critical_section::with(|cs| {
        let mut owner = FLASH.borrow_ref_mut(cs);
        let pt = read_partition_table(owner.as_mut()?, &mut table).ok()?;
        match pt.booted_partition().ok()??.partition_type() {
            PartitionType::App(AppPartitionSubType::Ota0) => Some(0),
            PartitionType::App(AppPartitionSubType::Ota1) => Some(1),
            _ => None,
        }
    })
}
