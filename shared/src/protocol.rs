//! Line-based text protocol spoken over the device's USB serial port.
//!
//! Requests (host -> device), one per line:
//! - `GET <led>`             → read the color an LED is showing
//! - `SET <led> <r> <g> <b>` → set an LED's color; each channel is 0..=255
//! - `INFO <key>`            → read one hardware value (see [`InfoKey`])
//! - `BOOTSEL`               → reboot into the USB bootloader (drive `RPI-RP2`)
//!
//! Responses (device -> host), one per line:
//! - `COLOR <led> <r> <g> <b>` → the color the LED now shows
//! - `INFO <key> <value>`      → the value requested with `INFO`
//! - `BOOTLOADER`              → acknowledged; the device is rebooting
//! - `ERR`                     → the request was malformed
//!
//! `<led>` is `rgb` for the small RGB LED or `neo` for the big NeoPixel.
//! Lines are terminated by `\n`; a trailing `\r` is tolerated.

use core::fmt::{self, Write};

/// A color for an LED, one 8-bit value per channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const OFF: Rgb = Rgb::new(0, 0, 0);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// The LEDs on the board that the protocol can address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedId {
    /// The small RGB LED (wired to GPIO 16, 17, and 25).
    Rgb,
    /// The big onboard NeoPixel (WS2812 on GPIO 12).
    Neo,
}

impl LedId {
    /// Every addressable LED, in display order.
    pub const ALL: [LedId; 2] = [LedId::Rgb, LedId::Neo];

    /// The token used for this LED on the wire.
    pub const fn wire_name(self) -> &'static str {
        match self {
            LedId::Rgb => "rgb",
            LedId::Neo => "neo",
        }
    }

    fn parse(token: &str) -> Option<LedId> {
        LedId::ALL.into_iter().find(|id| id.wire_name() == token)
    }
}

/// A hardware value the device can report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoKey {
    /// RP2040 chip ID register (revision, part, and manufacturer).
    Chip,
    /// Milliseconds since boot.
    Uptime,
    /// Die temperature in hundredths of a degree Celsius.
    Temperature,
    /// System clock frequency in Hz.
    SysClock,
    /// Version of the RP2040's boot ROM.
    RomVersion,
    /// Size of the flash region the firmware can use, in bytes.
    FlashTotal,
    /// Bytes of flash the firmware image occupies.
    FlashUsed,
    /// Size of the SRAM region the firmware links into, in bytes.
    RamTotal,
    /// Bytes of SRAM used by static data (`.data` and `.bss`).
    RamStatic,
    /// Most bytes the stack has ever used, since boot.
    RamStackPeak,
}

impl InfoKey {
    /// Every key, in display order.
    pub const ALL: [InfoKey; 10] = [
        InfoKey::Chip,
        InfoKey::Uptime,
        InfoKey::Temperature,
        InfoKey::SysClock,
        InfoKey::RomVersion,
        InfoKey::FlashTotal,
        InfoKey::FlashUsed,
        InfoKey::RamTotal,
        InfoKey::RamStatic,
        InfoKey::RamStackPeak,
    ];

    /// The token used for this key on the wire.
    pub const fn wire_name(self) -> &'static str {
        match self {
            InfoKey::Chip => "chip",
            InfoKey::Uptime => "uptime",
            InfoKey::Temperature => "temp",
            InfoKey::SysClock => "clock",
            InfoKey::RomVersion => "rom",
            InfoKey::FlashTotal => "flash_total",
            InfoKey::FlashUsed => "flash_used",
            InfoKey::RamTotal => "ram_total",
            InfoKey::RamStatic => "ram_static",
            InfoKey::RamStackPeak => "ram_stack",
        }
    }

    fn parse(token: &str) -> Option<InfoKey> {
        InfoKey::ALL.into_iter().find(|k| k.wire_name() == token)
    }
}

/// A hardware value, tagged with what it measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InfoValue {
    Chip(u32),
    UptimeMs(u32),
    TemperatureCentiC(i32),
    SysClockHz(u32),
    RomVersion(u8),
    FlashTotalBytes(u32),
    FlashUsedBytes(u32),
    RamTotalBytes(u32),
    RamStaticBytes(u32),
    RamStackPeakBytes(u32),
}

impl InfoValue {
    /// The key this value answers.
    pub const fn key(self) -> InfoKey {
        match self {
            InfoValue::Chip(_) => InfoKey::Chip,
            InfoValue::UptimeMs(_) => InfoKey::Uptime,
            InfoValue::TemperatureCentiC(_) => InfoKey::Temperature,
            InfoValue::SysClockHz(_) => InfoKey::SysClock,
            InfoValue::RomVersion(_) => InfoKey::RomVersion,
            InfoValue::FlashTotalBytes(_) => InfoKey::FlashTotal,
            InfoValue::FlashUsedBytes(_) => InfoKey::FlashUsed,
            InfoValue::RamTotalBytes(_) => InfoKey::RamTotal,
            InfoValue::RamStaticBytes(_) => InfoKey::RamStatic,
            InfoValue::RamStackPeakBytes(_) => InfoKey::RamStackPeak,
        }
    }
}

/// A message sent from the host to the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    GetColor(LedId),
    SetColor(LedId, Rgb),
    Info(InfoKey),
    /// Reboot into the RP2040's USB bootloader (drive `RPI-RP2`).
    Bootloader,
}

/// A message sent from the device to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    Color(LedId, Rgb),
    Info(InfoValue),
    /// The device is about to reboot into the bootloader.
    Bootloader,
    Error,
}

impl Request {
    /// Encode into `buf` as one newline-terminated line.
    ///
    /// Returns the number of bytes written, or `fmt::Error` if `buf` is too small.
    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, fmt::Error> {
        let mut w = LineWriter::new(buf);
        match self {
            Request::GetColor(led) => writeln!(w, "GET {}", led.wire_name())?,
            Request::SetColor(led, c) => {
                writeln!(w, "SET {} {} {} {}", led.wire_name(), c.r, c.g, c.b)?
            }
            Request::Info(key) => writeln!(w, "INFO {}", key.wire_name())?,
            Request::Bootloader => writeln!(w, "BOOTSEL")?,
        }
        Ok(w.len())
    }

    /// Parse one line (the trailing newline is optional).
    pub fn decode(line: &[u8]) -> Option<Request> {
        let line = core::str::from_utf8(line).ok()?.trim();
        let mut parts = line.split_ascii_whitespace();
        let request = match parts.next()? {
            "GET" => Request::GetColor(parse_led(&mut parts)?),
            "SET" => {
                let led = parse_led(&mut parts)?;
                Request::SetColor(led, parse_rgb(&mut parts)?)
            }
            "INFO" => Request::Info(InfoKey::parse(parts.next()?)?),
            "BOOTSEL" => Request::Bootloader,
            _ => return None,
        };
        parts.next().is_none().then_some(request)
    }
}

impl Response {
    /// Encode into `buf` as one newline-terminated line.
    pub fn encode(&self, buf: &mut [u8]) -> Result<usize, fmt::Error> {
        let mut w = LineWriter::new(buf);
        match self {
            Response::Color(led, c) => {
                writeln!(w, "COLOR {} {} {} {}", led.wire_name(), c.r, c.g, c.b)?
            }
            Response::Info(value) => {
                let key = value.key().wire_name();
                let v: i64 = match *value {
                    InfoValue::Chip(v) => v.into(),
                    InfoValue::UptimeMs(v) => v.into(),
                    InfoValue::TemperatureCentiC(v) => v.into(),
                    InfoValue::SysClockHz(v) => v.into(),
                    InfoValue::RomVersion(v) => v.into(),
                    InfoValue::FlashTotalBytes(v)
                    | InfoValue::FlashUsedBytes(v)
                    | InfoValue::RamTotalBytes(v)
                    | InfoValue::RamStaticBytes(v)
                    | InfoValue::RamStackPeakBytes(v) => v.into(),
                };
                writeln!(w, "INFO {key} {v}")?
            }
            Response::Bootloader => writeln!(w, "BOOTLOADER")?,
            Response::Error => writeln!(w, "ERR")?,
        }
        Ok(w.len())
    }

    /// Parse one line (the trailing newline is optional).
    pub fn decode(line: &[u8]) -> Option<Response> {
        let line = core::str::from_utf8(line).ok()?.trim();
        let mut parts = line.split_ascii_whitespace();
        let response = match parts.next()? {
            "COLOR" => {
                let led = parse_led(&mut parts)?;
                Response::Color(led, parse_rgb(&mut parts)?)
            }
            "INFO" => {
                let key = InfoKey::parse(parts.next()?)?;
                let token = parts.next()?;
                let value = match key {
                    InfoKey::Chip => InfoValue::Chip(token.parse().ok()?),
                    InfoKey::Uptime => InfoValue::UptimeMs(token.parse().ok()?),
                    InfoKey::Temperature => InfoValue::TemperatureCentiC(token.parse().ok()?),
                    InfoKey::SysClock => InfoValue::SysClockHz(token.parse().ok()?),
                    InfoKey::RomVersion => InfoValue::RomVersion(token.parse().ok()?),
                    InfoKey::FlashTotal => InfoValue::FlashTotalBytes(token.parse().ok()?),
                    InfoKey::FlashUsed => InfoValue::FlashUsedBytes(token.parse().ok()?),
                    InfoKey::RamTotal => InfoValue::RamTotalBytes(token.parse().ok()?),
                    InfoKey::RamStatic => InfoValue::RamStaticBytes(token.parse().ok()?),
                    InfoKey::RamStackPeak => InfoValue::RamStackPeakBytes(token.parse().ok()?),
                };
                Response::Info(value)
            }
            "BOOTLOADER" => Response::Bootloader,
            "ERR" => Response::Error,
            _ => return None,
        };
        parts.next().is_none().then_some(response)
    }
}

fn parse_led<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Option<LedId> {
    LedId::parse(parts.next()?)
}

fn parse_rgb<'a>(parts: &mut impl Iterator<Item = &'a str>) -> Option<Rgb> {
    let r = parts.next()?.parse().ok()?;
    let g = parts.next()?.parse().ok()?;
    let b = parts.next()?.parse().ok()?;
    Some(Rgb::new(r, g, b))
}

/// A `fmt::Write` sink over a fixed byte buffer, used to encode lines
/// without allocating. Fails with `fmt::Error` instead of truncating.
struct LineWriter<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> LineWriter<'a> {
    fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl Write for LineWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len + s.len();
        if end > self.buf.len() {
            return Err(fmt::Error);
        }
        self.buf[self.len..end].copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let mut buf = [0u8; 32];
        for req in [
            Request::GetColor(LedId::Rgb),
            Request::GetColor(LedId::Neo),
            Request::SetColor(LedId::Rgb, Rgb::new(0, 128, 255)),
            Request::SetColor(LedId::Neo, Rgb::new(255, 255, 255)),
            Request::Info(InfoKey::Temperature),
            Request::Bootloader,
        ] {
            let n = req.encode(&mut buf).unwrap();
            assert_eq!(Request::decode(&buf[..n]), Some(req));
        }
    }

    #[test]
    fn response_round_trip() {
        let mut buf = [0u8; 32];
        for resp in [
            Response::Color(LedId::Neo, Rgb::new(1, 2, 3)),
            Response::Color(LedId::Rgb, Rgb::OFF),
            Response::Info(InfoValue::Chip(0x0000_0002)),
            Response::Info(InfoValue::UptimeMs(123_456)),
            Response::Info(InfoValue::TemperatureCentiC(-512)),
            Response::Info(InfoValue::SysClockHz(125_000_000)),
            Response::Info(InfoValue::RomVersion(3)),
            Response::Info(InfoValue::FlashTotalBytes(2 * 1024 * 1024)),
            Response::Info(InfoValue::FlashUsedBytes(40_960)),
            Response::Info(InfoValue::RamTotalBytes(256 * 1024)),
            Response::Info(InfoValue::RamStaticBytes(8_192)),
            Response::Info(InfoValue::RamStackPeakBytes(1_024)),
            Response::Bootloader,
            Response::Error,
        ] {
            let n = resp.encode(&mut buf).unwrap();
            assert_eq!(Response::decode(&buf[..n]), Some(resp));
        }
    }

    #[test]
    fn encodes_expected_wire_format() {
        let mut buf = [0u8; 32];
        let n = Request::SetColor(LedId::Neo, Rgb::new(10, 20, 30))
            .encode(&mut buf)
            .unwrap();
        assert_eq!(&buf[..n], b"SET neo 10 20 30\n");
        let n = Request::GetColor(LedId::Rgb).encode(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"GET rgb\n");
        let n = Request::Info(InfoKey::SysClock).encode(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"INFO clock\n");
        let n = Response::Color(LedId::Rgb, Rgb::new(5, 6, 7))
            .encode(&mut buf)
            .unwrap();
        assert_eq!(&buf[..n], b"COLOR rgb 5 6 7\n");
        let n = Response::Info(InfoValue::TemperatureCentiC(2534))
            .encode(&mut buf)
            .unwrap();
        assert_eq!(&buf[..n], b"INFO temp 2534\n");
        let n = Response::Error.encode(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"ERR\n");
    }

    #[test]
    fn tolerates_crlf_and_extra_spaces() {
        assert_eq!(
            Request::decode(b"  SET   neo 1 2 3\r"),
            Some(Request::SetColor(LedId::Neo, Rgb::new(1, 2, 3)))
        );
    }

    #[test]
    fn rejects_malformed_requests() {
        for line in [
            &b""[..],
            b"FOO",
            b"GET",
            b"GET strip",
            b"GET rgb extra",
            b"SET rgb 1 2",
            b"SET rgb 1 2 3 4",
            b"SET rgb 256 0 0",
            b"SET rgb -1 0 0",
            b"SET rgb a b c",
            b"SET 1 2 3",
            b"INFO",
            b"INFO voltage",
            b"INFO chip extra",
        ] {
            assert_eq!(Request::decode(line), None, "line: {line:?}");
        }
    }

    #[test]
    fn rejects_malformed_responses() {
        assert_eq!(Response::decode(b"COLOR rgb 1 2"), None);
        assert_eq!(Response::decode(b"COLOR zzz 1 2 3"), None);
        assert_eq!(Response::decode(b"INFO uptime -5"), None);
        assert_eq!(Response::decode(b"INFO rom 300"), None);
        assert_eq!(Response::decode(b"HELLO"), None);
    }

    #[test]
    fn encode_fails_when_buffer_too_small() {
        let mut buf = [0u8; 4];
        assert!(
            Request::SetColor(LedId::Neo, Rgb::new(255, 255, 255))
                .encode(&mut buf)
                .is_err()
        );
    }
}
