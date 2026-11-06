//! Rendering. Reads [`App`] and draws; never mutates state.

use micropc_shared::Rgb;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};

use crate::app::{App, LedPanel, PRESETS, led_title, preset_name};

/// Color of the border around the section that has keyboard focus.
const FOCUS_COLOR: Color = Color::Cyan;

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(
        Paragraph::new("micropc · LEDs")
            .bold()
            .centered()
            .block(Block::bordered()),
        header,
    );

    let sections = Layout::vertical([Constraint::Min(11), Constraint::Min(11)]).split(body);
    for (index, (panel, area)) in app.panels.iter().zip(sections.iter()).enumerate() {
        draw_panel(frame, panel, *area, index == app.focus);
    }

    let help = format!(
        " {}   ·   Tab switch LED  ↑/↓ select  Enter apply  r refresh  q quit",
        app.status
    );
    frame.render_widget(Line::from(help).dim(), footer);
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

fn to_color(c: Rgb) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}
