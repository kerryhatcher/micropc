//! TUI state and key handling. Independent of the terminal and the serial
//! port so it can be unit-tested against a fake [`Device`].

use micropc_shared::{LedId, Rgb};
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

pub struct App {
    /// One panel per LED, in display order.
    pub panels: [LedPanel; 2],
    /// Index into `panels` of the section that receives keys.
    pub focus: usize,
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
            panels: LedId::ALL.map(LedPanel::new),
            focus: 0,
            status: String::from("connected"),
            should_quit: false,
        }
    }

    /// Ask the device for every LED's current color.
    pub fn refresh(&mut self, device: &mut dyn Device) {
        for panel in &mut self.panels {
            match device.get_color(panel.id) {
                Ok(color) => panel.current = Some(color),
                Err(e) => {
                    self.status = format!("{} read failed: {e}", led_title(panel.id));
                }
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

    pub fn select_next(&mut self) {
        let panel = &mut self.panels[self.focus];
        panel.selected = (panel.selected + 1) % PRESETS.len();
    }

    pub fn select_prev(&mut self) {
        let panel = &mut self.panels[self.focus];
        panel.selected = (panel.selected + PRESETS.len() - 1) % PRESETS.len();
    }

    pub fn handle_key(&mut self, key: KeyCode, device: &mut dyn Device) {
        match key {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Tab => self.focus_next(),
            KeyCode::Up | KeyCode::Char('k') => self.select_prev(),
            KeyCode::Down | KeyCode::Char('j') => self.select_next(),
            KeyCode::Enter | KeyCode::Char(' ') => self.apply_selected(device),
            KeyCode::Char('r') => self.refresh(device),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device::DeviceError;
    use std::io;

    /// In-memory device with one color per LED. `fail` makes every call
    /// return an I/O error. `sets` records each (LED, color) written.
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
    fn tab_moves_focus_and_apply_targets_the_neopixel() {
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
    fn preset_name_matches_exact_colors_only() {
        assert_eq!(preset_name(Rgb::new(0, 255, 0)), Some("Green"));
        assert_eq!(preset_name(Rgb::new(1, 255, 0)), None);
    }
}
