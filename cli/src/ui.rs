//! Rendering. Reads [`App`] and draws; never mutates state.

use micropc_shared::Rgb;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph, Row, Table, Tabs},
};

use crate::app::{App, HardwareInfo, LedPanel, PRESETS, Tab, led_title, preset_name};

/// Color of the border around the section that has keyboard focus.
const FOCUS_COLOR: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let titles = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, tab)| format!("{} {}", i + 1, tab.title()));
    frame.render_widget(
        Tabs::new(titles)
            .select(app.tab.index())
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .block(Block::bordered().title(" micropc ")),
        header,
    );

    match app.tab {
        Tab::Leds => draw_leds(frame, app, body),
        Tab::Hardware => draw_hardware(frame, &app.hardware, body),
    }

    let keys = match app.tab {
        Tab::Leds => "Tab switch LED  ↑/↓ select  Enter apply  ←/→ tab  r refresh  q quit",
        Tab::Hardware => "←/→ tab  r refresh  q quit",
    };
    let help = format!(" {}   ·   {keys}", app.status);
    frame.render_widget(Line::from(help).dim(), footer);
}

/// The LED tab: one section per LED, stacked.
fn draw_leds(frame: &mut Frame, app: &App, area: Rect) {
    let sections = Layout::vertical([Constraint::Min(11), Constraint::Min(11)]).split(area);
    for (index, (panel, section)) in app.panels.iter().zip(sections.iter()).enumerate() {
        draw_panel(frame, panel, *section, index == app.focus);
    }
}

/// One LED's section: its current color on the left, its menu on the right.
fn draw_panel(frame: &mut Frame, panel: &LedPanel, area: Rect, focused: bool) {
    let border = if focused {
        Style::new().fg(FOCUS_COLOR)
    } else {
        Style::new()
    };
    let block = Block::bordered()
        .title(format!(" {} ", led_title(panel.id)))
        .border_style(border);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [status_area, menu_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(inner);
    draw_status(frame, panel, status_area);
    draw_menu(frame, panel, menu_area, focused);
}

/// The "current color" readout: a swatch plus the values the device reports.
fn draw_status(frame: &mut Frame, panel: &LedPanel, area: Rect) {
    let [swatch, details] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(area);

    match panel.current {
        Some(color) => {
            frame.render_widget(Block::new().style(Style::new().bg(to_color(color))), swatch);
            let lines = vec![
                Line::from(format!("Name: {}", preset_name(color).unwrap_or("custom"))).bold(),
                Line::from(format!("RGB:  {} {} {}", color.r, color.g, color.b)),
                Line::from(format!(
                    "Hex:  #{:02X}{:02X}{:02X}",
                    color.r, color.g, color.b
                )),
            ];
            frame.render_widget(Paragraph::new(lines), details);
        }
        None => {
            frame.render_widget(Paragraph::new("reading from device…").dim(), swatch);
        }
    }
}

/// The "set color" menu for one LED. The highlight only shows reversed when
/// this section has focus, so the active menu is easy to spot.
fn draw_menu(frame: &mut Frame, panel: &LedPanel, area: Rect, focused: bool) {
    let items: Vec<ListItem> = PRESETS
        .iter()
        .map(|preset| {
            // Off is black, which would vanish on a dark terminal; show it grey.
            let swatch = if preset.color == Rgb::OFF {
                Color::DarkGray
            } else {
                to_color(preset.color)
            };
            ListItem::new(Line::from(vec![
                Span::styled("■ ", Style::new().fg(swatch)),
                Span::raw(preset.name),
            ]))
        })
        .collect();

    let highlight = if focused {
        Style::new().add_modifier(Modifier::REVERSED)
    } else {
        Style::new().add_modifier(Modifier::BOLD)
    };
    let list = List::new(items)
        .highlight_style(highlight)
        .highlight_symbol("> ");

    let mut state = ListState::default().with_selected(Some(panel.selected));
    frame.render_stateful_widget(list, area, &mut state);
}

/// The hardware tab: a table of values read from the RP2040.
fn draw_hardware(frame: &mut Frame, hw: &HardwareInfo, area: Rect) {
    let rows = [
        row("Board", "Seeed Studio XIAO RP2040".into()),
        row(
            "Chip ID",
            hw.chip_id.map_or(unknown(), |v| format!("0x{v:08X}")),
        ),
        row(
            "ROM version",
            hw.rom_version.map_or(unknown(), |v| v.to_string()),
        ),
        row(
            "System clock",
            hw.sys_clock_hz.map_or(unknown(), |hz| {
                format!("{:.3} MHz", f64::from(hz) / 1_000_000.0)
            }),
        ),
        row("Uptime", hw.uptime_ms.map_or(unknown(), format_uptime)),
        row(
            "Die temperature",
            hw.temperature_centi_c.map_or(unknown(), format_temperature),
        ),
        row("Flash", usage(hw.flash_used, hw.flash_total)),
        row("RAM", usage(ram_in_use(hw), hw.ram_total)),
        row("  static data", hw.ram_static.map_or(unknown(), size)),
        row("  stack peak", hw.ram_stack_peak.map_or(unknown(), size)),
    ];

    let table = Table::new(rows, [Constraint::Length(18), Constraint::Min(10)])
        .header(Row::new(["Item", "Value"]).bold())
        .block(Block::bordered().title(" Hardware "));
    frame.render_widget(table, area);
}

fn row(label: &'static str, value: String) -> Row<'static> {
    Row::new([label.to_string(), value])
}

/// Static data plus the stack's peak use: the RAM the firmware has claimed.
fn ram_in_use(hw: &HardwareInfo) -> Option<u32> {
    Some(hw.ram_static? + hw.ram_stack_peak?)
}

/// `used` of `total`, with the free amount and percentage in use.
fn usage(used: Option<u32>, total: Option<u32>) -> String {
    match (used, total) {
        (Some(used), Some(total)) if total > 0 => {
            let free = total.saturating_sub(used);
            // Tenths of a percent, so small usage doesn't round to 0%.
            let tenths = u64::from(used) * 1000 / u64::from(total);
            format!(
                "{} used, {} free of {} ({}.{}%)",
                size(used),
                size(free),
                size(total),
                tenths / 10,
                tenths % 10
            )
        }
        _ => unknown(),
    }
}

/// Bytes as KiB or MiB with one decimal place.
fn size(bytes: u32) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let b = f64::from(bytes);
    if b >= MIB {
        format!("{:.1} MiB", b / MIB)
    } else {
        format!("{:.1} KiB", b / KIB)
    }
}

fn unknown() -> String {
    "…".to_string()
}

/// `h:mm:ss` from milliseconds since boot.
fn format_uptime(ms: u32) -> String {
    let total = ms / 1000;
    format!(
        "{}:{:02}:{:02}",
        total / 3600,
        (total / 60) % 60,
        total % 60
    )
}

/// Hundredths of a degree Celsius to `NN.NN °C`.
fn format_temperature(centi_c: i32) -> String {
    let sign = if centi_c < 0 { "-" } else { "" };
    let abs = centi_c.unsigned_abs();
    format!("{sign}{}.{:02} °C", abs / 100, abs % 100)
}

fn to_color(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uptime_formats_hours_minutes_seconds() {
        assert_eq!(format_uptime(0), "0:00:00");
        assert_eq!(format_uptime(4_200_000), "1:10:00");
        assert_eq!(format_uptime(3_661_000), "1:01:01");
    }

    #[test]
    fn size_picks_kib_or_mib() {
        assert_eq!(size(1024), "1.0 KiB");
        assert_eq!(size(40_960), "40.0 KiB");
        assert_eq!(size(2 * 1024 * 1024), "2.0 MiB");
    }

    #[test]
    fn usage_shows_used_free_and_percent() {
        assert_eq!(
            usage(Some(256 * 1024), Some(1024 * 1024)),
            "256.0 KiB used, 768.0 KiB free of 1.0 MiB (25.0%)"
        );
        assert_eq!(usage(None, Some(1024)), "…");
    }

    #[test]
    fn ram_in_use_sums_static_and_stack() {
        let hw = HardwareInfo {
            ram_static: Some(8_192),
            ram_stack_peak: Some(1_024),
            ..HardwareInfo::default()
        };
        assert_eq!(ram_in_use(&hw), Some(9_216));
        assert_eq!(ram_in_use(&HardwareInfo::default()), None);
    }

    #[test]
    fn temperature_formats_sign_and_hundredths() {
        assert_eq!(format_temperature(2534), "25.34 °C");
        assert_eq!(format_temperature(-512), "-5.12 °C");
        assert_eq!(format_temperature(5), "0.05 °C");
    }
}
