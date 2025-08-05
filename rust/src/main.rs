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

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    App::default().run(terminal)
}

/// The main application which holds the state and logic of the application.
pub struct App {
    state: AppState,
    selected_tab: SelectedTab,
    modules: BTreeMap<SelectedTab, Box<dyn Module>>,
}

impl Default for App {
    fn default() -> Self {
        let mut modules: BTreeMap<SelectedTab, Box<dyn Module>> = BTreeMap::new();

        modules.insert(SelectedTab::TabThermal, Box::new(Thermal::new()));
        modules.insert(SelectedTab::TabRTC, Box::new(Rtc::new()));
        modules.insert(SelectedTab::TabUCSI, Box::new(Ucsi::new()));
        modules.insert(SelectedTab::TabBattery, Box::new(Battery::new()));

        Self {
            state: Default::default(),
            selected_tab: Default::default(),
            modules,
        }
    }
}

/// Internal trait to be implemented by modules (or Tabs).
pub(crate) trait Module {
    /// Update the module.
    fn update(&mut self);

    /// Handle input event.
    fn handle_event(&mut self, evt: &Event);

    /// Render the module.
    fn render(&self, area: Rect, buf: &mut Buffer);
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AppState {
    #[default]
    Running,
    Quitting,
}

#[derive(Default, Clone, Copy, Display, FromRepr, EnumIter, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum SelectedTab {
    #[default]
    #[strum(to_string = "Battery")]
    TabBattery,
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

        self.update_tabs();
        while self.state == AppState::Running {
            terminal.draw(|frame| frame.render_widget(&self, frame.area()))?;

            // Adjust timeout to account for delay from handling input
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());

            // Handle event if we got it, and only update tab states if we timed out
            if event::poll(timeout)? {
                self.handle_events()?;
            } else {
                self.update_tabs();
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

                    // Let the current tab handle event in this case
                    _ => self.handle_tab_event(&evt),
                }
            }
        }
        Ok(())
    }

    fn handle_tab_event(&mut self, evt: &Event) {
        self.modules
            .get_mut(&self.selected_tab)
            .expect("Tab must exist")
            .handle_event(evt);
    }

    fn update_tabs(&mut self) {
        for module in self.modules.values_mut() {
            module.update();
        }
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
        self.render_selected_tab(inner_area, buf);
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

    fn render_selected_tab(&self, area: Rect, buf: &mut Buffer) {
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
        self.modules
            .get(&self.selected_tab)
            .expect("Battery must exist")
            .render(inner, buf);
    }

    fn render_thermal(&self, area: Rect, buf: &mut Buffer) {
        let block = self.selected_tab.block().title("Thermal Information");
        let inner = block.inner(area);

        block.render(area, buf);
        self.modules
            .get(&SelectedTab::TabThermal)
            .expect("Thermal must exist")
            .render(inner, buf);
    }

    fn render_rtc(&self, area: Rect, buf: &mut Buffer) {
        let block = self.selected_tab.block().title("RTC Information");
        let inner = block.inner(area);

        block.render(area, buf);
        self.modules
            .get(&self.selected_tab)
            .expect("RTC must exist")
            .render(inner, buf);
    }

    fn render_ucsi(&self, area: Rect, buf: &mut Buffer) {
        let block = self.selected_tab.block().title("UCSI Information");
        let inner = block.inner(area);

        block.render(area, buf);
        self.modules
            .get(&self.selected_tab)
            .expect("UCSI must exist")
            .render(inner, buf);
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
