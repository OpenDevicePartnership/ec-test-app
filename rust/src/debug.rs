use crate::Source;
use crate::app::Module;
use crate::common;
use crate::notifications;
use clap::{Parser, Subcommand};
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
    widgets::{Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};
use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;
use tui_input::{Input, backend::crossterm::EventHandler};

type ReadFrameResult = Result<Option<Vec<Line<'static>>>>;
type DefmtDecoder<'a> = Box<dyn StreamDecoder + 'a>;

const MAX_LOGS: usize = 1000;

#[derive(Parser)]
#[command(name = "dbg-cmd", disable_help_subcommand = true)]
struct Cmd {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    Attach { path: String },
    Detach,
    Help,
}

#[derive(Default)]
struct CmdHandler {
    input: Input,
}

impl CmdHandler {
    fn parse(&mut self, line: String) -> Result<Action> {
        // TODO: Will likely need to check if the command is something that should be passed to debug service
        // As in, should differentiate between commands that affect the TUI vs affect the debug service
        let tokens = line.split_whitespace();
        Ok(Cmd::try_parse_from(std::iter::once("dbg-cmd").chain(tokens))
            .map_err(|_| eyre!("Invalid command"))?
            .action)
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let width = area.width.max(3) - 3;
        let scroll = self.input.visual_scroll(width as usize);

        let input = Paragraph::new(self.input.value())
            .style(Style::default())
            .scroll((0, scroll as u16))
            .block(Block::bordered().title("Command <ENTER>"));
        input.render(area, buf);
    }
}

// The defmt_decoder API requires stream_decoder to hold a reference to Table
// The stream_decoder is stateful so needs to be stored alongside Table in struct
// This creates a self-referential struct which is tricky hence the use of self_cell
self_cell::self_cell! {
    struct DefmtDecoderCell {
        owner: Table,
        #[not_covariant]
        dependent: DefmtDecoder,
    }
}

struct DefmtHandler {
    bin_name: String,
    decoder: DefmtDecoderCell,
}

impl DefmtHandler {
    pub fn new(elf_path: PathBuf) -> Result<Self> {
        let bin_name = elf_path
            .file_name()
            .ok_or(eyre!("No file name found in ELF path"))?
            .to_str()
            .ok_or(eyre!("Invalid ELF path"))?
            .to_owned();
        let elf = fs::read(elf_path).map_err(|_| eyre!("Failed to read ELF"))?;

        let table = Table::parse(&elf)
            .map_err(|e| eyre!(e))?
            .ok_or(eyre!("ELF contains no `.defmt` section"))?;
        let decoder = DefmtDecoderCell::new(table, |table| table.new_stream_decoder());

        Ok(Self { bin_name, decoder })
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
        for span in msg.iter().skip(1) {
            lines.push(Line::raw(format!("{:pad$}{span}", "", pad = ts_len + 7)));
        }
        lines
    }

    fn read_log(&mut self, raw: Vec<u8>) -> ReadFrameResult {
        self.decoder.with_dependent_mut(|_, d| d.received(&raw));

        // TODO: May want to keep looping until reach EOF since we could receive multiple frames since last update
        // However current debug service appears to guarantee only a single full frame will be sent at a time
        match self.decoder.with_dependent_mut(|_, d| d.decode()) {
            Ok(f) => Ok(Some(Self::frame2lines(&f))),
            Err(DecodeError::UnexpectedEof) => Ok(None),
            Err(DecodeError::Malformed) => Err(eyre!("Received malformed defmt packet")),
        }
    }
}

struct ScrollState {
    bar: ScrollbarState,
    pos: usize,
    size: u16,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            bar: Default::default(),
            pos: 0,
            size: u16::MAX,
        }
    }
}

#[derive(Default)]
struct LogView {
    y_scroll: ScrollState,
    x_scroll: ScrollState,
    max_log_len: usize,
    logs: common::SampleBuf<Line<'static>, MAX_LOGS>,
}

impl LogView {
    // Updates cached logs with newly read frame
    fn log_frame(&mut self, frame: ReadFrameResult) {
        match frame {
            // If a full frame was received, log it
            Ok(Some(log)) => {
                let lines = log.len();
                for line in log {
                    let len = format!("{line}").len();
                    self.max_log_len = std::cmp::max(self.max_log_len, len);
                    self.logs.insert(line);
                }
                self.update_scroll(lines);
            }
            // Unless it was an error
            // TODO: Handle recovery?
            Err(e) => {
                self.log_meta(e);
            }
            // But if was unexpected EOF, just do nothing until we get the full frame
            _ => {}
        }
    }

    fn log_meta(&mut self, msg: impl std::fmt::Display) {
        self.logs
            .insert(Line::styled(format!("<{msg}>"), Style::default().fg(Color::Cyan)));
        self.update_scroll(1);
    }

    fn scroll_up(&mut self) {
        self.y_scroll.pos = self.y_scroll.pos.saturating_sub(1);
        self.y_scroll.bar.prev();
    }

    fn scroll_down(&mut self) {
        if self.logs.len() > self.y_scroll.size as usize {
            self.y_scroll.pos = self
                .y_scroll
                .pos
                .saturating_add(1)
                .clamp(0, self.logs.len() - self.y_scroll.size as usize);
            self.y_scroll.bar.next();
        }
    }

    fn scroll_left(&mut self) {
        self.x_scroll.pos = self.x_scroll.pos.saturating_sub(1);
        self.x_scroll.bar.prev();
    }

    fn scroll_right(&mut self) {
        if self.max_log_len > self.x_scroll.size as usize {
            self.x_scroll.pos = self
                .x_scroll
                .pos
                .saturating_add(1)
                .clamp(0, self.max_log_len - self.x_scroll.size as usize);
            self.x_scroll.bar.next();
        }
    }

    // Updates log pane scroll state
    fn update_scroll(&mut self, new_lines: usize) {
        // Adjust the length of the horizontal scroll bar if a log doesn't fit in the window
        if self.max_log_len > self.x_scroll.size as usize {
            self.x_scroll.bar = self
                .x_scroll
                .bar
                .content_length(self.max_log_len - self.x_scroll.size as usize);
        }

        // Adjust the length of the vertical scroll bar if the number of logs doesn't fit in the window
        if self.logs.len() > self.y_scroll.size as usize {
            let height = self.logs.len() - self.y_scroll.size as usize;
            self.y_scroll.bar = self.y_scroll.bar.content_length(height);

            // If we are currently scrolled to the bottom, stay scrolled to the bottom as new logs come in
            if self.y_scroll.pos == height.saturating_sub(new_lines) {
                self.y_scroll.bar = self.y_scroll.bar.position(height);
                self.y_scroll.pos = height;
            }
        }
    }

    fn display_help(&mut self) {
        let help_lines: [&'static str; 4] = [
            "Commands supported:",
            "help (Display help)",
            "attach <elf-path> (Attach an ELF file to view defmt logs)",
            "detach (Detach ELF)",
        ];

        for line in help_lines {
            self.logs.insert(Line::raw(line));
        }
        self.update_scroll(4);
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Separate this from paragraph because we need to know the inner area for proper log scrolling
        let b = common::title_block("Logs (Use Shift + ◄ ▲ ▼ ► to scroll)", 1, Color::White);
        self.y_scroll.size = b.inner(area).height;
        self.x_scroll.size = b.inner(area).width;

        Paragraph::new(self.logs.as_vec())
            .scroll((self.y_scroll.pos as u16, self.x_scroll.pos as u16))
            .block(b)
            .render(area, buf);

        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .render(area, buf, &mut self.y_scroll.bar);
        Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
            .begin_symbol(Some("◄"))
            .end_symbol(Some("►"))
            .thumb_symbol("🬋")
            .render(area, buf, &mut self.x_scroll.bar);
    }
}

pub struct Debug<S: Source> {
    // Currently source is unused by main thread, but keeping it for ease of use in future
    source: S,
    log_view: LogView,
    defmt: Option<DefmtHandler>,
    cmd_handler: CmdHandler,
    event_rx: notifications::EventRx<Result<Vec<u8>>>,
}

impl<S: Source> Module for Debug<S> {
    fn title(&self) -> Cow<'static, str> {
        format!(
            "Debug Information ({})",
            self.defmt.as_ref().map(|d| d.bin_name.as_str()).unwrap_or("None")
        )
        .into()
    }

    fn update(&mut self) {
        if let Some(defmt) = &mut self.defmt {
            while let Some(data) = self.event_rx.receive() {
                match data {
                    Ok(raw) => {
                        let frame = defmt.read_log(raw);
                        self.log_view.log_frame(frame);
                    }
                    Err(e) => self.log_view.log_meta(e),
                }
            }
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Give logs area as much room as possible
        let [logs_area, cmd_area] =
            common::area_split_constrained(area, Direction::Vertical, Constraint::Min(0), Constraint::Max(3));

        self.log_view.render(logs_area, buf);
        self.cmd_handler.render(cmd_area, buf);
    }

    fn handle_event(&mut self, evt: &Event) {
        if let Event::Key(key) = evt
            && key.kind == KeyEventKind::Press
        {
            match key.code {
                KeyCode::Up => self.log_view.scroll_up(),
                KeyCode::Down => self.log_view.scroll_down(),
                KeyCode::Left => self.log_view.scroll_left(),
                KeyCode::Right => self.log_view.scroll_right(),
                KeyCode::Enter => {
                    let str = self.cmd_handler.input.value_and_reset();
                    self.handle_cmd(str);
                }
                _ => {
                    let _ = self.cmd_handler.input.handle_event(evt);
                }
            }
        }
    }
}

impl<S: Source> Debug<S> {
    pub fn new(source: S, elf_path: Option<PathBuf>, notifications: &notifications::Notifications) -> Self {
        // Sources must ensure they are thread-safe
        // Currently mock and ACPI are thread-safe
        let src = source.clone();

        // Reads the raw defmt frame from source every time notification is received and stores in buffer
        // Previously, the event receiver just queued up bools when notifications were received so tabs could poll that
        // But, not particularly effective for debug tab since the debug service will only send a new notification after the previous has been acknowledged
        // This resulted in the debug tab only getting a single debug frame once a second which is too slow
        // So instead the event receiver thread itself will call `get_dbg` as notifications come in and store the raw frames in a buffer
        // The debug tab can then just process every raw frame once a second and push all those to the log viewer
        // This allows for a more real-time approach of receiving logs
        let event_rx =
            notifications.event_receiver(notifications::Event::DbgFrameAvailable, move |_event| src.get_dbg());

        let mut debug = Self {
            source,
            log_view: Default::default(),
            defmt: None,
            cmd_handler: Default::default(),
            event_rx,
        };

        if let Some(elf_path) = elf_path {
            debug.attach_elf(elf_path);
        } else {
            debug.detach_elf();

            #[cfg(feature = "mock")]
            debug.log_view.log_meta("Try running the command `attach mock-bin`");
        }

        debug
    }

    fn handle_cmd(&mut self, str: String) {
        match self.cmd_handler.parse(str) {
            Ok(action) => match action {
                Action::Attach { path } => self.attach_elf(PathBuf::from(path)),
                Action::Detach => self.detach_elf(),
                Action::Help => self.log_view.display_help(),
            },
            Err(e) => self.log_view.log_meta(e),
        }
    }

    fn attach_elf(&mut self, elf_path: PathBuf) {
        match DefmtHandler::new(elf_path) {
            Ok(defmt) => {
                self.log_view.log_meta(format!("Attached ELF: {}", defmt.bin_name));
                self.defmt = Some(defmt);
                self.event_rx.start();

                // Initial read to kick off debug service (since we would've missed last notification)
                let _ = self.source.get_dbg();
            }
            Err(e) => {
                self.defmt = None;
                self.log_view.log_meta(format!("Failed to attach ELF: {}", e));
            }
        }
    }

    fn detach_elf(&mut self) {
        self.defmt = None;
        self.log_view
            .log_meta("No ELF attached so debug logs are not available");
        self.event_rx.stop();
    }
}
