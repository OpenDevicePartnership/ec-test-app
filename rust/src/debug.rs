use crate::Source;
use crate::app::Module;
use crate::common;
use color_eyre::eyre::Result;
use color_eyre::eyre::eyre;
use crossterm::event::{KeyCode, KeyEventKind};
use defmt_decoder::{DecodeError, Frame, StreamDecoder, Table};
use ratatui::{
    buffer::Buffer,
    crossterm::event::Event,
    layout::{Constraint, Direction, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph, Scrollbar, ScrollbarState, StatefulWidget, Widget},
};
use std::fs;
use std::path::PathBuf;
use tui_input::{Input, backend::crossterm::EventHandler};

type ReadFrameResult = Result<Option<Vec<Line<'static>>>>;

const MAX_LOGS: usize = 1000;

struct DefmtDecoder {
    decoder: Box<dyn StreamDecoder>,
}

impl DefmtDecoder {
    fn new(elf_path: PathBuf) -> Result<Self> {
        let elf = fs::read(elf_path).map_err(|_| eyre!("Failed to read ELF"))?;
        let table = Box::leak(Box::new(
            Table::parse(&elf)
                .map_err(|_| eyre!("Invalid ELF file"))?
                .ok_or(eyre!("ELF contains no `.defmt` section"))?,
        ));

        Ok(Self {
            decoder: table.new_stream_decoder(),
        })
    }

    fn level_color(level: &str) -> Color {
        match level {
            "TRACE" => Color::Gray,
            "DEBUG" => Color::White,
            "INFO" => Color::Green,
            "WARN" => Color::Yellow,
            "ERROR" => Color::Red,
            _ => Color::Black,
        }
    }

    // Unfortunately, the provided color formatter by defmt_decoder doesn't play nicely with Ratatui
    // Hence the need for this manual formatting with color
    fn frame2lines(f: &Frame) -> Vec<Line<'static>> {
        let msg = format!("{} ", f.display_message());
        let ts = f
            .display_timestamp()
            .map_or_else(|| " ".to_string(), |ts| format!("{ts} "));
        let ts_len = ts.len();
        let level = f
            .level()
            .map_or_else(|| " ".to_string(), |level| level.as_str().to_uppercase());

        // Have to match over the string since the `Level` enum type is not re-exported
        let level_color = Self::level_color(level.as_str());

        let ts = Span::raw(ts);
        let level = Span::styled(format!("{level:<7}"), Style::default().fg(level_color));

        // A log can be multiple lines, but ratatui won't automatically display a newline
        // Hence the need to manually split the log and create a `Line` for each
        let msg: Vec<Span<'_>> = msg.lines().map(|m| Span::raw(m.to_owned())).collect();

        // The first line will always contain timestamp, level, and first line of log
        let mut lines = vec![Line::from(vec![ts, level, msg[0].clone()])];

        // If there are additional lines in the log, add them here
        // We also align it with the first line of the log, just looks nicer
        msg.iter()
            .skip(1)
            .for_each(|s| lines.push(Line::raw(format!("{:pad$}{s}", "", pad = ts_len + 7))));

        lines
    }

    fn read_log(&mut self, raw: Vec<u8>) -> ReadFrameResult {
        self.decoder.received(&raw);

        // TODO: May want to keep looping until reach EOF since we could receive multiple frames since last update
        match self.decoder.decode() {
            Ok(f) => Ok(Some(Self::frame2lines(&f))),
            Err(DecodeError::UnexpectedEof) => Ok(None),
            Err(DecodeError::Malformed) => Err(eyre!("Malformed defmt packet!")),
        }
    }
}

#[derive(Default)]
struct ScrollState {
    bar: ScrollbarState,
    pos: usize,
    length: u16,
}

pub struct Debug<S: Source> {
    source: S,
    y_scroll: ScrollState,
    x_scroll: ScrollState,
    max_log_len: usize,
    decoder: Option<DefmtDecoder>,
    logs: common::SampleBuf<Line<'static>, MAX_LOGS>,
    input: Input,
    bin_name: String,
}

impl<S: Source> Module for Debug<S> {
    fn title(&self) -> String {
        format!("Debug Information ({})", self.bin_name)
    }

    fn update(&mut self) {
        if let Some(decoder) = &mut self.decoder {
            let raw = self.source.get_dbg_data().unwrap();

            let frame = decoder.read_log(raw);
            let lines = self.update_logs(frame);

            self.update_scroll(lines);
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Give logs area as much room as possible
        let [logs_area, cmd_area] =
            common::area_split_constrained(area, Direction::Vertical, Constraint::Min(0), Constraint::Max(3));

        // If no ELF attached, don't bother trying to render logs
        if self.decoder.is_some() {
            self.render_logs(logs_area, buf);
        } else {
            Paragraph::new("<No ELF file attached so debug logs are not available>").render(area, buf);
        }

        self.render_cmd_input(cmd_area, buf);
    }

    fn handle_event(&mut self, evt: &Event) {
        if let Event::Key(key) = evt
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Up => self.scroll_up(),
                KeyCode::Down => self.scroll_down(),
                KeyCode::Left => self.scroll_left(),
                KeyCode::Right => self.scroll_right(),
                KeyCode::Enter => {
                    let cmd = self.input.value_and_reset();
                    let _ = self.source.send_dbg_cmd(cmd);
                }
                _ => {
                    let _ = self.input.handle_event(evt);
                }
            }
        }
    }
}

impl<S: Source> Debug<S> {
    pub fn new(source: S, _elf_path: Option<PathBuf>) -> Result<Self> {
        // For mock, always use our predetermined mock-bin
        #[cfg(feature = "mock")]
        let _elf_path = Some(PathBuf::from("mock-bin"));

        let bin_name = if let Some(elf_path) = &_elf_path {
            elf_path
                .file_name()
                .ok_or(eyre!("No file name found in ELF path"))?
                .to_str()
                .ok_or(eyre!("Invalid ELF path"))?
                .to_owned()
        } else {
            "None".to_string()
        };
        let decoder = _elf_path.map(DefmtDecoder::new).transpose()?;

        Ok(Self {
            source,
            y_scroll: Default::default(),
            x_scroll: Default::default(),
            max_log_len: 0,
            decoder,
            logs: common::SampleBuf::default(),
            input: Default::default(),
            bin_name,
        })
    }

    fn scroll_up(&mut self) {
        self.y_scroll.pos = self.y_scroll.pos.saturating_sub(1);
        self.y_scroll.bar.prev();
    }

    fn scroll_down(&mut self) {
        if self.logs.len() > self.y_scroll.length as usize {
            self.y_scroll.pos = self
                .y_scroll
                .pos
                .saturating_add(1)
                .clamp(0, self.logs.len() - self.y_scroll.length as usize);
            self.y_scroll.bar.next();
        }
    }

    fn scroll_left(&mut self) {
        self.x_scroll.pos = self.x_scroll.pos.saturating_sub(1);
        self.x_scroll.bar.prev();
    }

    fn scroll_right(&mut self) {
        if self.max_log_len > self.x_scroll.length as usize {
            self.x_scroll.pos = self
                .x_scroll
                .pos
                .saturating_add(1)
                .clamp(0, self.max_log_len - self.x_scroll.length as usize);
            self.x_scroll.bar.next();
        }
    }

    fn render_logs(&mut self, area: Rect, buf: &mut Buffer) {
        // Separate this from paragraph because we need to know the inner area for proper log scrolling
        let b = common::title_block("Logs (Use Shift + ◄ ▲ ▼ ► to scroll)", 1, Color::White);
        self.y_scroll.length = b.inner(area).height;
        self.x_scroll.length = b.inner(area).width;

        Paragraph::new(self.logs.as_vec())
            .scroll((self.y_scroll.pos as u16, self.x_scroll.pos as u16))
            .block(b)
            .render(area, buf);

        Scrollbar::new(ratatui::widgets::ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .render(area, buf, &mut self.y_scroll.bar);
        Scrollbar::new(ratatui::widgets::ScrollbarOrientation::HorizontalBottom)
            .begin_symbol(Some("◄"))
            .end_symbol(Some("►"))
            .thumb_symbol("🬋")
            .render(area, buf, &mut self.x_scroll.bar);
    }

    fn render_cmd_input(&mut self, area: Rect, buf: &mut Buffer) {
        let width = area.width.max(3) - 3;
        let scroll = self.input.visual_scroll(width as usize);

        let input = Paragraph::new(self.input.value())
            .style(Style::default())
            .scroll((0, scroll as u16))
            .block(Block::bordered().title("Command <ENTER>"));
        input.render(area, buf);
    }

    // Updates cached logs with newly read frame, returns the number of lines inserted
    fn update_logs(&mut self, frame: ReadFrameResult) -> usize {
        // If a full frame was received, log it
        if let Ok(Some(log)) = frame {
            let lines = log.len();
            for line in log {
                let len = format!("{line}").len();
                self.max_log_len = std::cmp::max(self.max_log_len, len);
                self.logs.insert(line);
            }
            lines

        // Unless it was an error
        // TODO: Handle recovery?
        } else if frame.is_err() {
            self.logs.insert(Line::from("<Malformed defmt frame>"));
            1

        // But if was unexpected EOF, just do nothing until we get the full frame
        } else {
            0
        }
    }

    // Updates log pane scroll state
    fn update_scroll(&mut self, new_lines: usize) {
        // Adjust the length of the horizontal scroll bar if a log doesn't fit in the window
        if self.max_log_len > self.x_scroll.length as usize {
            self.x_scroll.bar = self
                .x_scroll
                .bar
                .content_length(self.max_log_len - self.x_scroll.length as usize);
        }

        // Adjust the length of the vertical scroll bar if the number of logs doesn't fit in the window
        if self.logs.len() > self.y_scroll.length as usize {
            let height = self.logs.len() - self.y_scroll.length as usize;
            self.y_scroll.bar = self.y_scroll.bar.content_length(height);

            // If we are currently scrolled to the bottom, stay scrolled to the bottom as new logs come in
            if self.y_scroll.pos == height - new_lines {
                self.y_scroll.bar = self.y_scroll.bar.position(height);
                self.y_scroll.pos = height;
            }
        }
    }
}
