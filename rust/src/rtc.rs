use std::borrow::Cow;

use crossterm::event::Event;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Stylize, palette::tailwind},
    text::Line,
    widgets::{Block, Borders, Padding, Paragraph, Widget},
};

use crate::app::Module;

const LABEL_COLOR: Color = tailwind::SLATE.c200;

#[derive(Default)]
pub struct Rtc {}

impl Module for Rtc {
    fn title(&self) -> Cow<'static, str> {
        "RTC Information".into()
    }

    fn update(&mut self) {}

    fn handle_event(&mut self, _evt: &Event) {}

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let status_title = title_block("RTC Properties");
        Paragraph::default().block(status_title).render(area, buf);
    }
}

impl Rtc {
    pub fn new() -> Self {
        Self {}
    }
}

fn title_block(title: &str) -> Block<'_> {
    let title = Line::from(title);
    Block::new()
        .borders(Borders::NONE)
        .padding(Padding::vertical(1))
        .title(title)
        .fg(LABEL_COLOR)
}
