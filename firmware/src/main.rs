//! RP2040 firmware for the Seeed XIAO RP2040: drives two LEDs and exposes
//! them over USB serial.
//!
//! - The small RGB LED (red GPIO17, green GPIO16, blue GPIO25, PWM).
//! - The big onboard NeoPixel (WS2812 on GPIO12, powered from GPIO11).
//!
//! The host sets and reads colors with the line protocol in
//! `micropc_shared::protocol`. The device is the source of truth; every
//! `GET` and `SET` reports the color the LED now shows. `INFO` reports
//! hardware values from the RP2040 itself.

#![no_std]
#![no_main]

mod hardware;
mod neopixel;

use cortex_m_rt::entry;
use embedded_hal::digital::OutputPin;
use embedded_hal::pwm::SetDutyCycle;
use hardware::Hardware;
use micropc_shared::{LedId, Request, Response, Rgb};
use neopixel::NeoPixel;
use panic_halt as _;
use rp2040_hal::{
    self as hal, clocks::init_clocks_and_plls, gpio::FunctionPio0, pac, pwm::Slices, usb::UsbBus,
    watchdog::Watchdog,
};
use usb_device::{
    UsbError,
    class_prelude::UsbBusAllocator,
    device::{StringDescriptors, UsbDevice, UsbDeviceBuilder, UsbVidPid},
};
use usbd_serial::{SerialPort, USB_CLASS_CDC};

/// Second-stage bootloader, copied to flash by the boot ROM.
#[unsafe(link_section = ".boot2")]
#[unsafe(no_mangle)]
#[used]
pub static BOOT2_FIRMWARE: [u8; 256] = rp2040_boot2::BOOT_LOADER_W25Q080;

/// External crystal frequency on the XIAO RP2040.
const XOSC_CRYSTAL_FREQ: u32 = 12_000_000;

/// Polarity of the small RGB LED. `true` means a channel lights when its pin
/// is driven low (common anode / active-low). Not verified on hardware yet:
/// if colors look inverted (red shows as cyan, etc.), flip this constant.
const COMMON_ANODE: bool = true;

/// PWM counter top value: each channel has 256 brightness levels.
const PWM_TOP: u16 = 255;

/// Longest request line accepted, excluding the newline.
const LINE_CAP: usize = 32;

/// USB test VID/PID (pid.codes). Must match `cli/src/main.rs`.
const USB_VID_PID: UsbVidPid = UsbVidPid(0x16c0, 0x27dd);

#[entry]
fn main() -> ! {
    // Before anything else uses the stack, so stack usage can be measured.
    hardware::paint_free_ram();

    let mut pac = pac::Peripherals::take().unwrap();

    let mut watchdog = Watchdog::new(pac.WATCHDOG);
    let clocks = init_clocks_and_plls(
        XOSC_CRYSTAL_FREQ,
        pac.XOSC,
        pac.CLOCKS,
        pac.PLL_SYS,
        pac.PLL_USB,
        &mut pac.RESETS,
        &mut watchdog,
    )
    .ok()
    .unwrap();

    let sio = hal::Sio::new(pac.SIO);
    let pins = hal::gpio::Pins::new(
        pac.IO_BANK0,
        pac.PADS_BANK0,
        sio.gpio_bank0,
        &mut pac.RESETS,
    );

    // Small RGB LED: red and green share PWM slice 0 (channels B and A),
    // blue is slice 4 channel B.
    let slices = Slices::new(pac.PWM, &mut pac.RESETS);
    let mut pwm0 = slices.pwm0;
    let mut pwm4 = slices.pwm4;
    pwm0.set_top(PWM_TOP);
    pwm4.set_top(PWM_TOP);
    pwm0.enable();
    pwm4.enable();
    pwm0.channel_a.output_to(pins.gpio16); // green
    pwm0.channel_b.output_to(pins.gpio17); // red
    pwm4.channel_b.output_to(pins.gpio25); // blue

    let rgb_led = RgbLed {
        red: pwm0.channel_b,
        green: pwm0.channel_a,
        blue: pwm4.channel_b,
    };

    // Big NeoPixel: power it from GPIO11, then hand GPIO12 to PIO0.
    let mut neo_power = pins.gpio11.into_push_pull_output();
    neo_power.set_high().unwrap();
    let _ = pins.gpio12.into_function::<FunctionPio0>();
    let neo = NeoPixel::new(pac.PIO0, &mut pac.RESETS);

    let mut lights = Lights {
        rgb: rgb_led,
        neo,
        rgb_color: Rgb::OFF,
        neo_color: Rgb::OFF,
    };
    lights.set(LedId::Rgb, Rgb::OFF);
    lights.set(LedId::Neo, Rgb::OFF);

    let mut hardware = Hardware::new(pac.SYSINFO, pac.ADC, pac.TIMER, &mut pac.RESETS, &clocks);

    let usb_bus = UsbBusAllocator::new(UsbBus::new(
        pac.USBCTRL_REGS,
        pac.USBCTRL_DPRAM,
        clocks.usb_clock,
        true,
        &mut pac.RESETS,
    ));
    let mut serial = SerialPort::new(&usb_bus);
    let mut usb_dev = UsbDeviceBuilder::new(&usb_bus, USB_VID_PID)
        .strings(&[StringDescriptors::default()
            .manufacturer("micropc")
            .product("micropc LEDs")
            .serial_number("0001")])
        .unwrap()
        .device_class(USB_CLASS_CDC)
        .build();

    let mut line = [0u8; LINE_CAP];
    let mut len = 0;
    let mut overflowed = false;

    loop {
        if !usb_dev.poll(&mut [&mut serial]) {
            continue;
        }

        let mut chunk = [0u8; 64];
        let n = match serial.read(&mut chunk) {
            Ok(n) => n,
            Err(_) => continue, // WouldBlock: nothing to read yet
        };

        for &byte in &chunk[..n] {
            if byte != b'\n' {
                if len < LINE_CAP {
                    line[len] = byte;
                    len += 1;
                } else {
                    overflowed = true;
                }
                continue;
            }

            let reply = if overflowed {
                Response::Error
            } else {
                match Request::decode(&line[..len]) {
                    Some(Request::GetColor(led)) => Response::Color(led, lights.color(led)),
                    Some(Request::SetColor(led, color)) => {
                        lights.set(led, color);
                        Response::Color(led, color)
                    }
                    Some(Request::Info(key)) => Response::Info(hardware.read(key)),
                    Some(Request::Bootloader) => Response::Bootloader,
                    None => Response::Error,
                }
            };
            len = 0;
            overflowed = false;

            let mut out = [0u8; 32];
            if let Ok(written) = reply.encode(&mut out) {
                write_all(&mut usb_dev, &mut serial, &out[..written]);
            }
            if reply == Response::Bootloader {
                reboot_into_bootloader();
            }
        }
    }
}

/// Both LEDs, plus the last color set on each.
struct Lights<R, G, B> {
    rgb: RgbLed<R, G, B>,
    neo: NeoPixel,
    rgb_color: Rgb,
    neo_color: Rgb,
}

impl<R, G, B> Lights<R, G, B>
where
    R: SetDutyCycle,
    G: SetDutyCycle,
    B: SetDutyCycle,
{
    fn color(&self, led: LedId) -> Rgb {
        match led {
            LedId::Rgb => self.rgb_color,
            LedId::Neo => self.neo_color,
        }
    }

    fn set(&mut self, led: LedId, color: Rgb) {
        match led {
            LedId::Rgb => {
                self.rgb.show(color);
                self.rgb_color = color;
            }
            LedId::Neo => {
                self.neo.show(color);
                self.neo_color = color;
            }
        }
    }
}

/// The three PWM channels that make up the small RGB LED.
struct RgbLed<R, G, B> {
    red: R,
    green: G,
    blue: B,
}

impl<R, G, B> RgbLed<R, G, B>
where
    R: SetDutyCycle,
    G: SetDutyCycle,
    B: SetDutyCycle,
{
    fn show(&mut self, color: Rgb) {
        let _ = self.red.set_duty_cycle(duty(color.r));
        let _ = self.green.set_duty_cycle(duty(color.g));
        let _ = self.blue.set_duty_cycle(duty(color.b));
    }
}

/// Map an 8-bit channel value to a PWM duty cycle, honoring polarity.
fn duty(value: u8) -> u16 {
    let level = u16::from(value) * PWM_TOP / 255;
    if COMMON_ANODE { PWM_TOP - level } else { level }
}

/// Reboot into the RP2040's USB bootloader (drive `RPI-RP2`). Does not return.
///
/// Waits briefly first so the `BOOTLOADER` acknowledgement reaches the host
/// before the serial port disappears.
fn reboot_into_bootloader() -> ! {
    /// About 40 ms at 125 MHz.
    const ACK_DELAY_CYCLES: u32 = 5_000_000;
    cortex_m::asm::delay(ACK_DELAY_CYCLES);
    hal::rom_data::reset_to_usb_boot(0, 0)
}

/// Write the whole buffer, pumping USB while the endpoint is busy.
///
/// Gives up after a bounded number of attempts so a host that stops reading
/// cannot stall the LED loop indefinitely.
fn write_all(
    usb_dev: &mut UsbDevice<'_, UsbBus>,
    serial: &mut SerialPort<'_, UsbBus>,
    mut data: &[u8],
) {
    const MAX_ATTEMPTS: u32 = 10_000;
    let mut attempts = 0;
    while !data.is_empty() && attempts < MAX_ATTEMPTS {
        attempts += 1;
        match serial.write(data) {
            Ok(n) => data = &data[n..],
            Err(UsbError::WouldBlock) => {
                usb_dev.poll(&mut [&mut *serial]);
            }
            Err(_) => return,
        }
    }
}
