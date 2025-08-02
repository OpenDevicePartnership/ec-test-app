#[cfg(not(feature = "mock"))]
mod acpi;
mod battery;
mod rtc;
mod thermal;
mod ucsi;

use battery::Battery;
use rtc::Rtc;
use thermal::Thermal;
use ucsi::Ucsi;

use color_eyre::Result;

use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEventKind},
    layout::{Constraint, Layout, Rect},
    style::{Color, Stylize, palette::tailwind},
    symbols,
    text::Line,
    widgets::{Block, Padding, Tabs, Widget},
};

use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

use std::time::{Duration, Instant};

fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    App::default().run(terminal)
}

/// The main application which holds the state and logic of the application.
#[derive(Default)]
pub struct App {
    state: AppState,
    selected_tab: SelectedTab,
    thermal: Thermal,
    // TODO: Add fields for other services as they are implemented
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AppState {
    #[default]
    Running,
    Quitting,
}

#[derive(Default, Clone, Copy, Display, FromRepr, EnumIter)]
enum SelectedTab {
    #[strum(to_string = "Battery")]
    TabBattery,
    #[default]
    #[strum(to_string = "Thermal")]
    TabThermal,
    #[strum(to_string = "RTC")]
    TabRTC,
    #[strum(to_string = "UCSI")]
    TabUCSI,
}

impl App {
    /// Construct a new instance of [`App`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Run the application's main loop.
    fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let tick_rate = Duration::from_millis(1000);
        let mut last_tick = Instant::now();

        self.update();
        while self.state == AppState::Running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;

            // Adjust timeout to account for delay from handling input
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());

            match event::poll(timeout) {
                // Handle input if we got it
                Ok(true) => {
                    self.handle_events()?;
                }
                // But only update state if we actually timed out
                Ok(false) => {
                    self.update();
                }
                Err(_e) => todo!(),
            }

            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
            }
        }

        Ok(())
    }

    fn handle_events(&mut self) -> std::io::Result<()> {
        let evt = event::read()?;
        if let Event::Key(key) = evt {
            if key.kind == KeyEventKind::Press {
                match key.code {
                    KeyCode::Char('l') | KeyCode::Right => self.next_tab(),
                    KeyCode::Char('h') | KeyCode::Left => self.previous_tab(),
                    KeyCode::Char('q') | KeyCode::Esc => self.quit(),
                    // Let the current tab handle input in this case
                    _ => self.handle_input(&evt),
                }
            }
        }
        Ok(())
    }

    pub fn next_tab(&mut self) {
        self.selected_tab = self.selected_tab.next();
    }

    pub fn previous_tab(&mut self) {
        self.selected_tab = self.selected_tab.previous();
    }

    pub fn quit(&mut self) {
        self.state = AppState::Quitting;
    }
}

impl Drop for App {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

impl SelectedTab {
    /// Get the previous tab, if there is no previous tab return the current tab.
    fn previous(self) -> Self {
        let current_index: usize = self as usize;
        let previous_index = current_index.saturating_sub(1);
        Self::from_repr(previous_index).unwrap_or(self)
    }

    /// Get the next tab, if there is no next tab return the current tab.
    fn next(self) -> Self {
        let current_index = self as usize;
        let next_index = current_index.saturating_add(1);
        Self::from_repr(next_index).unwrap_or(self)
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        use Constraint::{Length, Min};
        let vertical = Layout::vertical([Length(1), Min(0), Length(1)]);
        let [header_area, inner_area, footer_area] = vertical.areas(area);

        let horizontal = Layout::horizontal([Min(0), Length(20)]);
        let [tabs_area, title_area] = horizontal.areas(header_area);

        render_title(title_area, buf);
        self.render_tabs(tabs_area, buf);
        self.render_tab(inner_area, buf);
        render_footer(footer_area, buf);
    }
}

impl App {
    fn render_tabs(&self, area: Rect, buf: &mut Buffer) {
        let titles = SelectedTab::iter().map(SelectedTab::title);
        let highlight_style = (Color::default(), self.selected_tab.palette().c700);
        let selected_tab_index = self.selected_tab as usize;
        Tabs::new(titles)
            .highlight_style(highlight_style)
            .select(selected_tab_index)
            .padding("", "")
            .divider(" ")
            .render(area, buf);
    }

    fn render_tab(&self, area: Rect, buf: &mut Buffer) {
        match self.selected_tab {
            SelectedTab::TabBattery => self.render_battery(area, buf),
            SelectedTab::TabThermal => self.render_thermal(area, buf),
            SelectedTab::TabRTC => self.render_rtc(area, buf),
            SelectedTab::TabUCSI => self.render_ucsi(area, buf),
        }
    }

    fn render_battery(&self, area: Rect, buf: &mut Buffer) {
        let block = self.selected_tab.block().title("Battery Information");
        let inner = block.inner(area);

        block.render(area, buf);
        Battery::render(inner, buf);
    }

    fn render_thermal(&self, area: Rect, buf: &mut Buffer) {
        let block = self.selected_tab.block().title("Thermal Information");
        let inner = block.inner(area);

        block.render(area, buf);
        self.thermal.render(inner, buf);
    }

    fn render_rtc(&self, area: Rect, buf: &mut Buffer) {
        let block = self.selected_tab.block().title("RTC Information");
        let inner = block.inner(area);

        block.render(area, buf);
        Rtc::render(inner, buf);
    }

    fn render_ucsi(&self, area: Rect, buf: &mut Buffer) {
        let block = self.selected_tab.block().title("UCSI Information");
        let inner = block.inner(area);

        block.render(area, buf);
        Ucsi::render(inner, buf);
    }

    fn handle_input(&mut self, evt: &Event) {
        match self.selected_tab {
            SelectedTab::TabThermal => self.thermal.handle_input(evt),
            // TODO: Handle input for other tabs as they are implemented
            SelectedTab::TabBattery => {}
            SelectedTab::TabRTC => {}
            SelectedTab::TabUCSI => {}
        }
    }

    fn update(&mut self) {
        self.thermal.update();
        // TODO: Update other tabs as they are implemented
    }
}

fn render_title(area: Rect, buf: &mut Buffer) {
    "ODP EC Demo App".bold().render(area, buf);
}

fn render_footer(area: Rect, buf: &mut Buffer) {
    Line::raw("◄ ► to change tab | Press q to quit")
        .centered()
        .render(area, buf);
}

impl SelectedTab {
    /// Return tab's name as a styled `Line`
    fn title(self) -> Line<'static> {
        format!("  {self}  ")
            .fg(tailwind::SLATE.c200)
            .bg(self.palette().c900)
            .into()
    }

    /// A block surrounding the tab's content
    fn block(self) -> Block<'static> {
        Block::bordered()
            .border_set(symbols::border::PROPORTIONAL_TALL)
            .padding(Padding::uniform(1))
            .border_style(self.palette().c700)
    }

    const fn palette(self) -> tailwind::Palette {
        match self {
            Self::TabBattery => tailwind::BLUE,
            Self::TabThermal => tailwind::EMERALD,
            Self::TabRTC => tailwind::INDIGO,
            Self::TabUCSI => tailwind::RED,
        }
    }
}
