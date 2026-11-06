# micropc

A terminal computer on an embedded device.

## Layout

This is a Cargo workspace with three crates:

| Path        | Crate             | Purpose                                                        |
|-------------|-------------------|----------------------------------------------------------------|
| `shared/`   | `micropc-shared`  | `no_std` library shared by the host tool and the firmware      |
| `cli/`      | `micropc-cli`     | Host-side CLI/TUI (binary: `micropc`)                          |
| `firmware/` | `micropc-firmware`| RP2040 firmware (`thumbv6m-none-eabi`, `no_std`, `no_main`)    |

## Building

```sh
# Host crates (shared + cli): default workspace members
cargo build
cargo test
cargo run -p micropc-cli

# RP2040 firmware (cross-compiled; excluded from default builds)
cargo fw            # debug   -> target/thumbv6m-none-eabi/debug/micropc-firmware
cargo fw-release    # release -> target/thumbv6m-none-eabi/release/micropc-firmware
```

`cargo fw` and `cargo fw-release` are aliases defined in `.cargo/config.toml`.
The `thumbv6m-none-eabi` target is installed automatically from
`rust-toolchain.toml`.

Plain `cargo check --workspace` or `cargo clippy --workspace` will fail on the
firmware crate because it must be built for its embedded target. Use
`--exclude micropc-firmware` for host-wide checks, and
`cargo clippy -p micropc-firmware --target thumbv6m-none-eabi` for the firmware.

## Using the TUI

The firmware runs on a Seeed Studio XIAO RP2040 and exposes its LEDs over USB
serial. With the board plugged in:

```sh
cargo run -p micropc-cli                  # auto-detects the device
cargo run -p micropc-cli -- --port /dev/ttyACM0
```

The screen has one section per LED:

- **Small RGB LED** (the user-controllable red/green/blue LED)
- **Big NeoPixel** (the bright WS2812 between BOOT and RESET)

Each section shows the color the LED is currently showing (re-read from the
device every second) and a menu of preset colors. Keys: `Tab` switches the
focused LED, `↑/↓` or `j/k` select, `Enter`/`Space` apply, `r` refresh, and
`q`/`Esc` quit.

To reboot the board into its USB bootloader (drive `RPI-RP2`) without pressing
any buttons:

```sh
cargo run -p micropc-cli -- bootloader
```

The device enumerates with USB ID `16c0:27dd` (pid.codes test IDs). The same
IDs are hard-coded in `firmware/src/main.rs` and `cli/src/main.rs`; change
both if you pick your own.

## Serial protocol

Defined in `shared/src/protocol.rs` (`PROTOCOL_VERSION` 3). One line per
message, newline-terminated. `<led>` is `rgb` (small RGB LED) or `neo` (NeoPixel).

| Direction     | Line                      | Meaning                                  |
|---------------|---------------------------|------------------------------------------|
| host → device | `GET <led>`               | Read the LED's current color             |
| host → device | `SET <led> <r> <g> <b>`   | Set the LED color (0–255 each)           |
| host → device | `BOOTSEL`                 | Reboot into the USB bootloader           |
| device → host | `COLOR <led> <r> <g> <b>` | The color the LED now shows              |
| device → host | `BOOTLOADER`              | Acknowledged; the device is rebooting    |
| device → host | `ERR`                     | Request was malformed                    |

## Hardware

Pins are from the [Seeed XIAO RP2040 wiki](https://wiki.seeedstudio.com/XIAO-RP2040)
pin table. The board has four LEDs:

| LED                      | Control              | Pins / notes                                     |
|--------------------------|----------------------|--------------------------------------------------|
| Small RGB LED (red)      | PWM, active-low      | GPIO17 (slice 0 B)                               |
| Small RGB LED (green)    | PWM, active-low      | GPIO16 (slice 0 A)                               |
| Small RGB LED (blue)     | PWM, active-low      | GPIO25 (slice 4 B)                               |
| Big NeoPixel (WS2812B)   | PIO0, data           | Data GPIO12; power enable GPIO11 (driven high)   |
| Power LED               | Hardware only        | Not on a GPIO; always on while the board is powered |

The wiki says the single-color LEDs are active-low: the pin has to be pulled
low to light them. `COMMON_ANODE` in `firmware/src/main.rs` is `true` to match.
The power LED is always red, and it's the red light you see when no color is
set.

## Flashing

Build with `cargo fw-release`, then convert the ELF to UF2 with `elf2uf2-rs`
(install with `cargo install elf2uf2-rs --no-default-features`):

```sh
elf2uf2-rs target/thumbv6m-none-eabi/release/micropc-firmware micropc-firmware.uf2
```

Put the board in bootloader mode with `micropc bootloader`, or by holding
**BOOT**, tapping **RESET**, then releasing **BOOT**. Copy the UF2 onto the
`RPI-RP2` drive. The board reboots into the new firmware.
