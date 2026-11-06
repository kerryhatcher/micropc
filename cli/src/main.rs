//! `micropc`: terminal UI for the RP2040 LEDs, plus helpers for the device.
//!
//! Usage:
//! - `micropc [--port <path>]`       open the TUI
//! - `micropc bootloader [--port …]` reboot the device into its USB bootloader
//!
//! Without `--port`, the first USB serial port with the micropc firmware's
//! VID/PID is used.

mod app;
mod device;
mod ui;

use std::env;
use std::error::Error;
use std::io;
use std::time::{Duration, Instant};

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyEventKind};
use serialport::{SerialPort, SerialPortType};

use app::App;
use device::{Client, Device};

/// USB VID/PID the firmware enumerates with. Must match `firmware/src/main.rs`.
const USB_VID: u16 = 0x16c0;
const USB_PID: u16 = 0x27dd;

const BAUD: u32 = 115_200; // ignored by USB CDC, but required by the API
const IO_TIMEOUT: Duration = Duration::from_millis(500);
const REFRESH_EVERY: Duration = Duration::from_secs(1);

const USAGE: &str = "usage: micropc [--port <path>]\n       micropc bootloader [--port <path>]";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Tui,
    Bootloader,
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    command: Command,
    port: Option<String>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args(env::args().skip(1))?;

    match args.command {
        Command::Tui => {
            let mut device = open_device(args.port)?;
            let mut terminal = ratatui::init();
            let result = run(&mut terminal, &mut device);
            ratatui::restore();
            result.map_err(Into::into)
        }
        Command::Bootloader => {
            let mut device = open_device(args.port)?;
            device.enter_bootloader()?;
            println!("device is rebooting into the bootloader; the RPI-RP2 drive should appear");
            Ok(())
        }
    }
}

/// Parse command-line arguments (without the program name).
fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, Box<dyn Error>> {
    let mut command = Command::Tui;
    let mut port = None;
    let mut first = true;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "bootloader" if first => command = Command::Bootloader,
            "--port" | "-p" => {
                port = Some(args.next().ok_or("--port needs a value")?);
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument: {other}\n{USAGE}").into()),
        }
        first = false;
    }
    Ok(Args { command, port })
}

/// Open the device at `port`, or the first port with the micropc USB ID.
fn open_device(port: Option<String>) -> Result<Client<Box<dyn SerialPort>>, Box<dyn Error>> {
    let port_name = match port {
        Some(path) => path,
        None => find_device()?,
    };
    let port = serialport::new(&port_name, BAUD)
        .timeout(IO_TIMEOUT)
        .open()
        .map_err(|e| format!("failed to open {port_name}: {e}"))?;
    Ok(Client::new(port))
}

/// Finds the micropc device among the system's USB serial ports.
fn find_device() -> Result<String, Box<dyn Error>> {
    serialport::available_ports()?
        .into_iter()
        .find_map(|port| match port.port_type {
            SerialPortType::UsbPort(info) if info.vid == USB_VID && info.pid == USB_PID => {
                Some(port.port_name)
            }
            _ => None,
        })
        .ok_or_else(|| {
            format!("no micropc device found (USB {USB_VID:04x}:{USB_PID:04x}); pass --port <path>")
                .into()
        })
}

fn run(terminal: &mut DefaultTerminal, device: &mut dyn Device) -> io::Result<()> {
    let mut app = App::new();
    app.refresh(device);
    let mut last_refresh = Instant::now();

    while !app.should_quit {
        terminal.draw(|frame| ui::draw(frame, &app))?;

        if event::poll(Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            app.handle_key(key.code, device);
        }

        // Pick up color changes made by other clients or by the firmware itself.
        if last_refresh.elapsed() >= REFRESH_EVERY {
            app.refresh(device);
            last_refresh = Instant::now();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, Box<dyn Error>> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_args_opens_tui() {
        let args = parse(&[]).unwrap();
        assert_eq!(
            args,
            Args {
                command: Command::Tui,
                port: None
            }
        );
    }

    #[test]
    fn bootloader_subcommand_with_port() {
        let args = parse(&["bootloader", "--port", "/dev/ttyACM0"]).unwrap();
        assert_eq!(
            args,
            Args {
                command: Command::Bootloader,
                port: Some("/dev/ttyACM0".into())
            }
        );
    }

    #[test]
    fn bootloader_must_come_first() {
        assert!(parse(&["--port", "/dev/x", "bootloader"]).is_err());
    }

    #[test]
    fn unknown_argument_is_an_error() {
        assert!(parse(&["--nope"]).is_err());
    }

    #[test]
    fn port_flag_needs_value() {
        assert!(parse(&["--port"]).is_err());
    }
}
