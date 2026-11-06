//! Types and protocol shared between the micropc CLI/TUI (host) and the
//! RP2040 firmware (`no_std`).
//!
//! Keep this crate `no_std` and dependency-free so it builds for both the
//! host and `thumbv6m-none-eabi`.

#![no_std]

pub mod protocol;

pub use protocol::{InfoKey, InfoValue, LedId, Request, Response, Rgb};

/// Version of the wire protocol between the host tool and the firmware.
/// Bump this on any breaking change to the message layout.
/// v2: requests and replies name the LED they address (`rgb` or `neo`).
/// v3: adds `BOOTSEL` to reboot into the USB bootloader.
/// v4: adds `INFO` to read hardware values (chip, uptime, temp, clock, ROM).
/// v5: adds flash and RAM usage keys (`flash_total`, `flash_used`, ...).
pub const PROTOCOL_VERSION: u8 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_is_nonzero() {
        assert_ne!(PROTOCOL_VERSION, 0);
    }
}
