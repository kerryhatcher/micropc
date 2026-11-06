//! Reads the RP2040's own hardware values for the `INFO` command, including
//! how much flash and SRAM the firmware uses.

use core::ptr::{read_volatile, write_volatile};

use micropc_shared::{InfoKey, InfoValue};
use rp2040_hal::{Adc, Clock, Timer, adc::TempSense, clocks::ClocksManager, pac, rom_data};

/// Start of the RP2040's flash in the address space.
const FLASH_BASE: usize = 0x1000_0000;
/// Flash size on the XIAO RP2040 (2 MiB QSPI, as in `firmware/memory.x`).
const FLASH_TOTAL: u32 = 2 * 1024 * 1024;

/// Start of SRAM.
const RAM_BASE: usize = 0x2000_0000;
/// SRAM the linker places the firmware in (`RAM` in `firmware/memory.x`).
const RAM_TOTAL: u32 = 256 * 1024;

/// Pattern written into unused RAM at boot, used to find the stack's low point.
const STACK_PAINT: u32 = 0xC0DE_CAFE;

unsafe extern "C" {
    // Provided by cortex-m-rt's linker script. Only their addresses are used.
    static __sidata: u32;
    static __sdata: u32;
    static __edata: u32;
    static __ebss: u32;
    static _stack_start: u32;
}

/// Fill the free RAM below the current stack pointer with a known pattern.
///
/// Call once, early in `main`. Afterwards [`Hardware::read`] can report how
/// far the stack has grown since boot. Only addresses below the stack pointer
/// are written, so no live data is touched.
pub fn paint_free_ram() {
    let start = (&raw const __ebss) as usize;
    let sp = cortex_m::register::msp::read() as usize;
    let mut addr = start;
    while addr + 4 <= sp {
        // SAFETY: `addr` is in SRAM, below the stack pointer, and 4-byte aligned.
        unsafe { write_volatile(addr as *mut u32, STACK_PAINT) };
        addr += 4;
    }
}

/// Lowest address the stack has reached, found by scanning up from the end of
/// static RAM for the first word that isn't still painted.
fn stack_low_water_mark() -> usize {
    let top = (&raw const _stack_start) as usize;
    let mut addr = (&raw const __ebss) as usize;
    while addr < top {
        // SAFETY: `addr` is inside SRAM between `__ebss` and the stack top.
        if unsafe { read_volatile(addr as *const u32) } != STACK_PAINT {
            break;
        }
        addr += 4;
    }
    addr
}

pub struct Hardware {
    adc: Adc,
    temp: TempSense,
    timer: Timer,
    chip_id: u32,
    sys_clock_hz: u32,
    /// Last successful temperature reading, reused if an ADC read fails.
    last_temp_centi_c: i32,
}

impl Hardware {
    pub fn new(
        sysinfo: pac::SYSINFO,
        adc: pac::ADC,
        timer: pac::TIMER,
        resets: &mut pac::RESETS,
        clocks: &ClocksManager,
    ) -> Self {
        let mut adc = Adc::new(adc, resets);
        let temp = adc
            .take_temp_sensor()
            .expect("temperature sensor is available once after ADC init");
        Self {
            adc,
            temp,
            timer: Timer::new(timer, resets, clocks),
            chip_id: sysinfo.chip_id().read().bits(),
            sys_clock_hz: clocks.system_clock.freq().to_Hz(),
            last_temp_centi_c: 0,
        }
    }

    /// Read one value.
    pub fn read(&mut self, key: InfoKey) -> InfoValue {
        match key {
            InfoKey::Chip => InfoValue::Chip(self.chip_id),
            InfoKey::Uptime => {
                let ms = self.timer.get_counter().ticks() / 1_000;
                InfoValue::UptimeMs(ms as u32)
            }
            InfoKey::Temperature => InfoValue::TemperatureCentiC(self.temperature_centi_c()),
            InfoKey::SysClock => InfoValue::SysClockHz(self.sys_clock_hz),
            InfoKey::RomVersion => InfoValue::RomVersion(rom_data::rom_version_number()),
            InfoKey::FlashTotal => InfoValue::FlashTotalBytes(FLASH_TOTAL),
            InfoKey::FlashUsed => InfoValue::FlashUsedBytes(flash_used()),
            InfoKey::RamTotal => InfoValue::RamTotalBytes(RAM_TOTAL),
            InfoKey::RamStatic => InfoValue::RamStaticBytes(ram_static()),
            InfoKey::RamStackPeak => InfoValue::RamStackPeakBytes(ram_stack_peak()),
        }
    }

    /// Die temperature in hundredths of a degree Celsius.
    ///
    /// RP2040 datasheet: T = 27 − (V − 0.706) / 0.001721, with V in volts.
    /// The ADC is 12 bits over a 3.3 V reference. Integer math avoids floats.
    fn temperature_centi_c(&mut self) -> i32 {
        if let Ok(raw) = self.adc.read(&mut self.temp) {
            let mv = i32::from(raw) * 3300 / 4095;
            self.last_temp_centi_c = 2700 - (mv - 706) * 100_000 / 1721;
        }
        self.last_temp_centi_c
    }
}

/// Bytes of flash the firmware image occupies, counting from the start of flash.
///
/// The image ends where the `.data` initializer ends in flash. Everything
/// else (code, read-only data, and the initializer) sits before that point.
fn flash_used() -> u32 {
    let data_len = (&raw const __edata) as usize - (&raw const __sdata) as usize;
    let image_end = (&raw const __sidata) as usize + data_len;
    (image_end - FLASH_BASE) as u32
}

/// Bytes of SRAM taken by static data: `.data` plus `.bss`.
fn ram_static() -> u32 {
    ((&raw const __ebss) as usize - RAM_BASE) as u32
}

/// Most stack the firmware has used since [`paint_free_ram`] ran.
fn ram_stack_peak() -> u32 {
    let top = (&raw const _stack_start) as usize;
    (top - stack_low_water_mark()) as u32
}
