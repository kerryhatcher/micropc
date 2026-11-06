//! TUI state and key handling. Independent of the terminal and the serial
//! port so it can be unit-tested against a fake [`Device`].

use micropc_shared::{InfoKey, InfoValue, LedId, Rgb};
use ratatui::crossterm::event::KeyCode;

use crate::device::Device;

/// A named color offered in the menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub color: Rgb,
}

/// Colors offered in each LED's menu, in display order.
pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Off",
        color: Rgb::new(0, 0, 0),
    },
    Preset {
        name: "Red",
        color: Rgb::new(255, 0, 0),
    },
    Preset {
        name: "Orange",
        color: Rgb::new(255, 128, 0),
    },
    Preset {
        name: "Yellow",
        color: Rgb::new(255, 255, 0),
    },
    Preset {
        name: "Green",
        color: Rgb::new(0, 255, 0),
    },
    Preset {
        name: "Cyan",
        color: Rgb::new(0, 255, 255),
    },
    Preset {
        name: "Blue",
        color: Rgb::new(0, 0, 255),
    },
    Preset {
        name: "Magenta",
        color: Rgb::new(255, 0, 255),
    },
    Preset {
        name: "White",
        color: Rgb::new(255, 255, 255),
    },
];

/// Name of the preset whose color matches exactly, if any.
pub fn preset_name(color: Rgb) -> Option<&'static str> {
    PRESETS.iter().find(|p| p.color == color).map(|p| p.name)
}

/// Human-readable name for an LED, shown as the section title.
pub fn led_title(led: LedId) -> &'static str {
    match led {
        LedId::Rgb => "Small RGB LED",
        LedId::Neo => "Big NeoPixel",
    }
}

/// The top-level tabs, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Leds,
    Hardware,
}

impl Tab {
    pub const ALL: [Tab; 2] = [Tab::Leds, Tab::Hardware];

    pub const fn title(self) -> &'static str {
        match self {
            Tab::Leds => "LEDs",
            Tab::Hardware => "Hardware",
        }
    }

    /// Position in [`Tab::ALL`].
    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// One LED's section: its last-read color and its menu selection.
pub struct LedPanel {
    pub id: LedId,
    /// The color last reported by the device. `None` until the first read.
    pub current: Option<Rgb>,
    /// Index into [`PRESETS`] of the highlighted menu entry.
    pub selected: usize,
}

impl LedPanel {
    fn new(id: LedId) -> Self {
        Self {
            id,
            current: None,
            selected: 0,
        }
    }
}

/// The most recent hardware readings. Each field is `None` until read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HardwareInfo {
    pub chip_id: Option<u32>,
    pub uptime_ms: Option<u32>,
    pub temperature_centi_c: Option<i32>,
    pub sys_clock_hz: Option<u32>,
    pub rom_version: Option<u8>,
    /// Flash the firmware can use, in bytes.
    pub flash_total: Option<u32>,
    /// Flash the firmware image occupies, in bytes.
    pub flash_used: Option<u32>,
    /// SRAM the firmware links into, in bytes.
    pub ram_total: Option<u32>,
    /// SRAM used by static data, in bytes.
    pub ram_static: Option<u32>,
    /// Most stack the firmware has used since boot, in bytes.
    pub ram_stack_peak: Option<u32>,
}

impl HardwareInfo {
    fn set(&mut self, value: InfoValue) {
        match value {
            InfoValue::Chip(v) => self.chip_id = Some(v),
            InfoValue::UptimeMs(v) => self.uptime_ms = Some(v),
            InfoValue::TemperatureCentiC(v) => self.temperature_centi_c = Some(v),
            InfoValue::SysClockHz(v) => self.sys_clock_hz = Some(v),
            InfoValue::RomVersion(v) => self.rom_version = Some(v),
            InfoValue::FlashTotalBytes(v) => self.flash_total = Some(v),
            InfoValue::FlashUsedBytes(v) => self.flash_used = Some(v),
            InfoValue::RamTotalBytes(v) => self.ram_total = Some(v),
            InfoValue::RamStaticBytes(v) => self.ram_static = Some(v),
            InfoValue::RamStackPeakBytes(v) => self.ram_stack_peak = Some(v),
        }
    }
}

pub struct App {
    pub tab: Tab,
    /// One panel per LED, in display order.
    pub panels: [LedPanel; 2],
    /// Index into `panels` of the LED section that receives keys.
    pub focus: usize,
    pub hardware: HardwareInfo,
    /// One-line message shown in the footer.
    pub status: String,
    pub should_quit: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            tab: Tab::Leds,
            panels: LedId::ALL.map(LedPanel::new),
            focus: 0,
            hardware: HardwareInfo::default(),
            status: String::from("connected"),
            should_quit: false,
        }
    }

    /// Ask the device for every LED's color and every hardware value.
    pub fn refresh(&mut self, device: &mut dyn Device) {
        for panel in &mut self.panels {
            match device.get_color(panel.id) {
                Ok(color) => panel.current = Some(color),
                Err(e) => {
                    self.status = format!("{} read failed: {e}", led_title(panel.id));
                }
            }
        }
        for key in InfoKey::ALL {
            match device.get_info(key) {
                Ok(value) => self.hardware.set(value),
                Err(e) => self.status = format!("hardware read failed: {e}"),
            }
        }
    }

    /// Send the focused panel's highlighted preset to the device.
    pub fn apply_selected(&mut self, device: &mut dyn Device) {
        let panel = &mut self.panels[self.focus];
        let preset = PRESETS[panel.selected];
        match device.set_color(panel.id, preset.color) {
            Ok(shown) => {
                panel.current = Some(shown);
                self.status = format!("{} set to {}", led_title(panel.id), preset.name);
            }
            Err(e) => {
                self.status = format!("{} set failed: {e}", led_title(panel.id));
            }
        }
    }

    pub fn focus_next(&mut self) {
        self.focus = (self.focus + 1) % self.panels.len();
    }

    pub fn select_tab(&mut self, tab: Tab) {
        self.tab = tab;
    }

    pub fn next_tab(&mut self) {
        let next = (self.tab.index() + 1) % Tab::ALL.len();
        self.tab = Tab::ALL[next];
    }

    pub fn prev_tab(&mut self) {
        let prev = (self.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len();
        self.tab = Tab::ALL[prev];
    }

    pub fn select_next(&mut self) {
        let panel = &mut self.panels[self.focus];
        panel.selected = (panel.selected + 1) % PRESETS.len();
    }

    pub fn select_prev(&mut self) {
        let panel = &mut self.panels[self.focus];
        panel.selected = (panel.selected + PRESETS.len() - 1) % PRESETS.len();
    }

    pub fn handle_key(&mut self, key: KeyCode, device: &mut dyn Device) {
        // Keys that work on every tab.
        match key {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
                return;
            }
            KeyCode::Char('1') => return self.select_tab(Tab::Leds),
            KeyCode::Char('2') => return self.select_tab(Tab::Hardware),
            KeyCode::Right | KeyCode::Char('l') => return self.next_tab(),
            KeyCode::Left | KeyCode::Char('h') => return self.prev_tab(),
            KeyCode::Char('r') => return self.refresh(device),
            _ => {}
        }

        // Keys that only make sense on the LED tab.
        if self.tab != Tab::Leds {
            return;
        }
        match key {
            KeyCode::Tab => self.focus_next(),
            KeyCode::Up | KeyCode::Char('k') => self.select_prev(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Enter | KeyCode::Char(' ') => self.apply_selected(device),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceError;
    use std::io;

    /// In-memory device with one color per LED and fixed hardware values.
    /// `fail` makes every call return an I/O error. `sets` records each
    /// (LED, color) written.
    struct FakeDevice {
        rgb: Rgb,
        neo: Rgb,
        fail: bool,
        sets: Vec<(LedId, Rgb)>,
    }

    impl FakeDevice {
        fn new() -> Self {
            Self {
                rgb: Rgb::OFF,
                neo: Rgb::OFF,
                fail: false,
                sets: Vec::new(),
            }
        }

        fn slot(&mut self, led: LedId) -> &mut Rgb {
            match led {
                LedId::Rgb => &mut self.rgb,
                LedId::Neo => &mut self.neo,
            }
        }

        fn err() -> DeviceError {
            DeviceError::Io(io::Error::other("unplugged"))
        }
    }

    impl Device for FakeDevice {
        fn get_color(&mut self, led: LedId) -> Result<Rgb, DeviceError> {
            if self.fail {
                return Err(Self::err());
            }
            Ok(*self.slot(led))
        }

        fn set_color(&mut self, led: LedId, color: Rgb) -> Result<Rgb, DeviceError> {
            if self.fail {
                return Err(Self::err());
            }
            self.sets.push((led, color));
            *self.slot(led) = color;
            Ok(color)
        }

        fn get_info(&mut self, key: InfoKey) -> Result<InfoValue, DeviceError> {
            if self.fail {
                return Err(Self::err());
            }
            Ok(match key {
                InfoKey::Chip => InfoValue::Chip(0x1234_5678),
                InfoKey::Uptime => InfoValue::UptimeMs(4_200),
                InfoKey::Temperature => InfoValue::TemperatureCentiC(2534),
                InfoKey::SysClock => InfoValue::SysClockHz(125_000_000),
                InfoKey::RomVersion => InfoValue::RomVersion(3),
                InfoKey::FlashTotal => InfoValue::FlashTotalBytes(2 * 1024 * 1024),
                InfoKey::FlashUsed => InfoValue::FlashUsedBytes(40_960),
                InfoKey::RamTotal => InfoValue::RamTotalBytes(256 * 1024),
                InfoKey::RamStatic => InfoValue::RamStaticBytes(8_192),
                InfoKey::RamStackPeak => InfoValue::RamStackPeakBytes(1_024),
            })
        }

        fn enter_bootloader(&mut self) -> Result<(), DeviceError> {
            Err(Self::err())
        }
    }

    #[test]
    fn refresh_reads_every_led() {
        let mut dev = FakeDevice::new();
        dev.rgb = Rgb::new(1, 2, 3);
        dev.neo = Rgb::new(4, 5, 6);
        let mut app = App::new();
        assert_eq!(app.panels[0].current, None);
        app.refresh(&mut dev);
        assert_eq!(app.panels[0].current, Some(Rgb::new(1, 2, 3)));
        assert_eq!(app.panels[1].current, Some(Rgb::new(4, 5, 6)));
    }

    #[test]
    fn refresh_reads_hardware_values() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        app.refresh(&mut dev);
        assert_eq!(app.hardware.chip_id, Some(0x1234_5678));
        assert_eq!(app.hardware.uptime_ms, Some(4_200));
        assert_eq!(app.hardware.temperature_centi_c, Some(2534));
        assert_eq!(app.hardware.sys_clock_hz, Some(125_000_000));
        assert_eq!(app.hardware.rom_version, Some(3));
        assert_eq!(app.hardware.flash_total, Some(2 * 1024 * 1024));
        assert_eq!(app.hardware.flash_used, Some(40_960));
        assert_eq!(app.hardware.ram_total, Some(256 * 1024));
        assert_eq!(app.hardware.ram_static, Some(8_192));
        assert_eq!(app.hardware.ram_stack_peak, Some(1_024));
    }

    #[test]
    fn apply_sends_focused_leds_preset() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        app.handle_key(KeyCode::Down, &mut dev); // Red on the small LED
        app.handle_key(KeyCode::Enter, &mut dev);
        assert_eq!(dev.sets, vec![(LedId::Rgb, Rgb::new(255, 0, 0))]);
        assert_eq!(app.panels[0].current, Some(Rgb::new(255, 0, 0)));
        assert_eq!(app.status, "Small RGB LED set to Red");
    }

    #[test]
    fn tab_moves_led_focus_and_apply_targets_the_neopixel() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        app.handle_key(KeyCode::Tab, &mut dev);
        assert_eq!(app.focus, 1);
        app.handle_key(KeyCode::Down, &mut dev);
        app.handle_key(KeyCode::Down, &mut dev); // Orange
        app.handle_key(KeyCode::Enter, &mut dev);
        assert_eq!(dev.sets, vec![(LedId::Neo, Rgb::new(255, 128, 0))]);
        assert_eq!(app.panels[1].current, Some(Rgb::new(255, 128, 0)));
        assert_eq!(app.panels[0].current, None);
    }

    #[test]
    fn each_panel_keeps_its_own_selection() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        app.handle_key(KeyCode::Down, &mut dev);
        app.handle_key(KeyCode::Tab, &mut dev);
        assert_eq!(app.panels[0].selected, 1);
        assert_eq!(app.panels[1].selected, 0);
    }

    #[test]
    fn navigation_wraps_in_both_directions() {
        let mut app = App::new();
        app.select_prev();
        assert_eq!(app.panels[0].selected, PRESETS.len() - 1);
        app.select_next();
        assert_eq!(app.panels[0].selected, 0);
    }

    #[test]
    fn failed_set_keeps_previous_color_and_reports_error() {
        let mut dev = FakeDevice::new();
        dev.rgb = Rgb::new(9, 9, 9);
        let mut app = App::new();
        app.refresh(&mut dev);
        dev.fail = true;
        app.apply_selected(&mut dev);
        assert_eq!(app.panels[0].current, Some(Rgb::new(9, 9, 9)));
        assert!(
            app.status.starts_with("Small RGB LED set failed"),
            "{}",
            app.status
        );
    }

    #[test]
    fn quit_keys_set_flag() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        app.handle_key(KeyCode::Char('q'), &mut dev);
        assert!(app.should_quit);
    }

    #[test]
    fn tabs_switch_with_arrows_and_digits() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        assert_eq!(app.tab, Tab::Leds);
        app.handle_key(KeyCode::Right, &mut dev);
        assert_eq!(app.tab, Tab::Hardware);
        app.handle_key(KeyCode::Right, &mut dev);
        assert_eq!(app.tab, Tab::Leds, "wraps around");
        app.handle_key(KeyCode::Char('2'), &mut dev);
        assert_eq!(app.tab, Tab::Hardware);
        app.handle_key(KeyCode::Char('1'), &mut dev);
        assert_eq!(app.tab, Tab::Leds);
        app.handle_key(KeyCode::Left, &mut dev);
        assert_eq!(app.tab, Tab::Hardware, "wraps backwards too");
    }

    #[test]
    fn led_keys_do_nothing_on_the_hardware_tab() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        app.handle_key(KeyCode::Char('2'), &mut dev);
        app.handle_key(KeyCode::Down, &mut dev);
        app.handle_key(KeyCode::Enter, &mut dev);
        app.handle_key(KeyCode::Tab, &mut dev);
        assert!(dev.sets.is_empty());
        assert_eq!(app.panels[0].selected, 0);
        assert_eq!(app.focus, 0);
    }

    #[test]
    fn refresh_works_on_every_tab() {
        let mut dev = FakeDevice::new();
        let mut app = App::new();
        app.handle_key(KeyCode::Char('2'), &mut dev);
        app.handle_key(KeyCode::Char('r'), &mut dev);
        assert_eq!(app.hardware.chip_id, Some(0x1234_5678));
    }

    #[test]
    fn preset_name_matches_exact_colors_only() {
        assert_eq!(preset_name(Rgb::new(0, 255, 0)), Some("Green"));
        assert_eq!(preset_name(Rgb::new(1, 255, 0)), None);
    }
}
