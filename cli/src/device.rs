//! Talks to the device over any byte stream (in practice, a USB serial port).

use std::fmt;
use std::io::{self, Read, Write};

use micropc_shared::{LedId, Request, Response, Rgb};

/// The operations the rest of the TUI needs from the device.
pub trait Device {
    /// Read the color an LED is currently showing.
    fn get_color(&mut self, led: LedId) -> Result<Rgb, DeviceError>;
    /// Set an LED's color. Returns the color the device reports it now shows.
    fn set_color(&mut self, led: LedId, color: Rgb) -> Result<Rgb, DeviceError>;
    /// Reboot the device into its USB bootloader (drive `RPI-RP2`).
    fn enter_bootloader(&mut self) -> Result<(), DeviceError>;
}

#[derive(Debug)]
pub enum DeviceError {
    Io(io::Error),
    /// The device replied with something that isn't a valid response line,
    /// or a reply about a different LED than the one asked for.
    BadResponse(String),
    /// The device replied `ERR` to a request it could not handle.
    Rejected,
}

impl fmt::Display for DeviceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DeviceError::Io(e) => write!(f, "I/O error: {e}"),
            DeviceError::BadResponse(line) => write!(f, "unexpected reply: {line:?}"),
            DeviceError::Rejected => write!(f, "device rejected the request"),
        }
    }
}

impl std::error::Error for DeviceError {}

impl From<io::Error> for DeviceError {
    fn from(e: io::Error) -> Self {
        DeviceError::Io(e)
    }
}

/// Maximum reply length we'll accept before giving up on a line.
const MAX_REPLY: usize = 64;

/// A [`Device`] that speaks the line protocol over `T`.
pub struct Client<T> {
    io: T,
}

impl<T> Client<T> {
    pub fn new(io: T) -> Self {
        Self { io }
    }
}

impl<T: Read + Write> Client<T> {
    /// Send `request` and return the device's reply.
    fn transact(&mut self, request: Request) -> Result<Response, DeviceError> {
        let mut buf = [0u8; 32];
        let n = request
            .encode(&mut buf)
            .map_err(|_| DeviceError::BadResponse("request too long".into()))?;
        self.io.write_all(&buf[..n])?;
        self.io.flush()?;

        let line = self.read_line()?;
        Response::decode(&line)
            .ok_or_else(|| DeviceError::BadResponse(String::from_utf8_lossy(&line).into_owned()))
    }

    /// Read bytes up to and including `\n`. Stops at `MAX_REPLY` bytes.
    fn read_line(&mut self) -> Result<Vec<u8>, DeviceError> {
        let mut line = Vec::with_capacity(MAX_REPLY);
        let mut byte = [0u8; 1];
        loop {
            self.io.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                return Ok(line);
            }
            if line.len() >= MAX_REPLY {
                return Err(DeviceError::BadResponse(
                    String::from_utf8_lossy(&line).into_owned(),
                ));
            }
            line.push(byte[0]);
        }
    }

    /// Turn a reply into the color for `led`, rejecting replies about other LEDs.
    fn color_reply(&mut self, request: Request, led: LedId) -> Result<Rgb, DeviceError> {
        match self.transact(request)? {
            Response::Color(reply_led, color) if reply_led == led => Ok(color),
            Response::Color(reply_led, _) => Err(DeviceError::BadResponse(format!(
                "reply for {} to a request for {}",
                reply_led.wire_name(),
                led.wire_name()
            ))),
            Response::Error => Err(DeviceError::Rejected),
            other => Err(DeviceError::BadResponse(format!("{other:?}"))),
        }
    }
}

impl<T: Read + Write> Device for Client<T> {
    fn get_color(&mut self, led: LedId) -> Result<Rgb, DeviceError> {
        self.color_reply(Request::GetColor(led), led)
    }

    fn set_color(&mut self, led: LedId, color: Rgb) -> Result<Rgb, DeviceError> {
        self.color_reply(Request::SetColor(led, color), led)
    }

    fn enter_bootloader(&mut self) -> Result<(), DeviceError> {
        match self.transact(Request::Bootloader)? {
            Response::Bootloader => Ok(()),
            Response::Error => Err(DeviceError::Rejected),
            other => Err(DeviceError::BadResponse(format!("{other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Scripted stream: reads from `reply`, records everything written.
    struct MockIo {
        reply: Cursor<Vec<u8>>,
        written: Vec<u8>,
    }

    impl MockIo {
        fn replying(reply: &[u8]) -> Self {
            Self {
                reply: Cursor::new(reply.to_vec()),
                written: Vec::new(),
            }
        }
    }

    impl Read for MockIo {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.reply.read(buf)
        }
    }

    impl Write for MockIo {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.written.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn get_color_parses_reply() {
        let mut client = Client::new(MockIo::replying(b"COLOR neo 9 8 7\n"));
        assert_eq!(client.get_color(LedId::Neo).unwrap(), Rgb::new(9, 8, 7));
        assert_eq!(client.io.written, b"GET neo\n");
    }

    #[test]
    fn set_color_sends_request_and_returns_reported_color() {
        let mut client = Client::new(MockIo::replying(b"COLOR rgb 0 255 0\n"));
        let shown = client.set_color(LedId::Rgb, Rgb::new(0, 255, 0)).unwrap();
        assert_eq!(shown, Rgb::new(0, 255, 0));
        assert_eq!(client.io.written, b"SET rgb 0 255 0\n");
    }

    #[test]
    fn reply_for_other_led_is_bad_response() {
        let mut client = Client::new(MockIo::replying(b"COLOR rgb 1 2 3\n"));
        assert!(matches!(
            client.get_color(LedId::Neo),
            Err(DeviceError::BadResponse(_))
        ));
    }

    #[test]
    fn err_reply_is_rejected() {
        let mut client = Client::new(MockIo::replying(b"ERR\n"));
        assert!(matches!(
            client.get_color(LedId::Rgb),
            Err(DeviceError::Rejected)
        ));
    }

    #[test]
    fn garbage_reply_is_bad_response() {
        let mut client = Client::new(MockIo::replying(b"HELLO WORLD\n"));
        assert!(matches!(
            client.get_color(LedId::Rgb),
            Err(DeviceError::BadResponse(_))
        ));
    }

    #[test]
    fn overlong_reply_is_bad_response() {
        let mut reply = vec![b'x'; MAX_REPLY + 10];
        reply.push(b'\n');
        let mut client = Client::new(MockIo::replying(&reply));
        assert!(matches!(
            client.get_color(LedId::Rgb),
            Err(DeviceError::BadResponse(_))
        ));
    }

    #[test]
    fn closed_connection_is_io_error() {
        let mut client = Client::new(MockIo::replying(b""));
        assert!(matches!(
            client.get_color(LedId::Rgb),
            Err(DeviceError::Io(_))
        ));
    }
}
