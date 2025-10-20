use crate::debug::Debug;
use crate::notifications::Notifications;
use crate::rtc::Rtc;
use crate::thermal::Thermal;
use crate::ucsi::Ucsi;
use crate::{Source, battery::Battery};

use color_eyre::Result;

use clap::Parser;
use ratatui::{
    DefaultTerminal,
    buffer::Buffer,
    crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    layout::{Constraint, Layout, Rect},
    style::{Color, Stylize, palette::tailwind},
    symbols,
    text::Line,
    widgets::{Block, Padding, Tabs, Widget},
};

use std::borrow::Cow;
use std::marker::PhantomData;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

use strum::{Display, EnumIter, FromRepr, IntoEnumIterator};

/// Internal trait to be implemented by modules (or Tabs).
pub(crate) trait Module {
    /// The module's title.
    fn title(&self) -> Cow<'static, str>;

    /// Update the module.
    fn update(&mut self);

    /// Handle input event.
    fn handle_event(&mut self, evt: &Event);

    /// Render the module.
    fn render(&mut self, area: Rect, buf: &mut Buffer);
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
    #[strum(to_string = "Debug")]
    TabDebug,
    #[strum(to_string = "Battery")]
    TabBattery,
    #[strum(to_string = "Thermal")]
    TabThermal,
    #[strum(to_string = "RTC")]
    TabRTC,
    #[strum(to_string = "UCSI")]
    TabUCSI,
}

#[derive(Parser)]
#[command(
    name = "ODP End-to-End Demo",
    version,
    about = "TUI application demonstrating Host-to-EC communication using Rust-based ODP components"
)]
pub struct AppArgs {
    #[arg(
        long,
        help = "Path to ELF file containing debug symbols and defmt strings currently running on EC"
    )]
    bin: Option<String>,
}

/// The main application which holds the state and logic of the application.
pub struct App<S: Source> {
    state: AppState,
    selected_tab: SelectedTab,
    modules: BTreeMap<SelectedTab, Box<dyn Module>>,
    phantom: PhantomData<S>,
}

impl<S: Source + Clone + 'static> App<S> {
    /// Construct a new instance of [`App`].
    pub fn new(source: S, args: AppArgs, notifications: &Notifications) -> Self {
        let mut modules: BTreeMap<SelectedTab, Box<dyn Module>> = BTreeMap::new();
        let source = Rc::new(RefCell::new(source));

        let thermal_source = Rc::clone(&source);
        let battery_source = Rc::clone(&source);
        let debug_source = Rc::clone(&source);

        modules.insert(
            SelectedTab::TabThermal,
            Box::new(Thermal::new(thermal_source.borrow().clone())),
        );
        modules.insert(SelectedTab::TabRTC, Box::new(Rtc::new()));
        modules.insert(SelectedTab::TabUCSI, Box::new(Ucsi::new()));
        modules.insert(
            SelectedTab::TabBattery,
            Box::new(Battery::new(battery_source.borrow().clone())),
        );
        modules.insert(
            SelectedTab::TabDebug,
            Box::new(Debug::new(
                debug_source.borrow().clone(),
                args.bin.as_ref().map(std::path::PathBuf::from),
                notifications,
            )),
        );

        Self {
            state: Default::default(),
            selected_tab: Default::default(),
            modules,
            phantom: PhantomData,
        }
    }

    /// Run the application's main loop.
    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        let tick_rate = Duration::from_millis(1000);
        let mut last_tick = Instant::now();

        while self.state == AppState::Running {
            terminal.draw(|frame| {
                frame.render_widget(&mut self, frame.area());
            })?;

            // Adjust timeout to account for delay from handling input
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());

            // Handle event if we got it, and only update tab states if we timed out
            if event::poll(timeout)? {
                self.handle_events()?;
            }

            if last_tick.elapsed() >= tick_rate {
                self.update_tabs();
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
                    // Check for shift modifier so tabs can still make use of Shift+Arrow Key
                    KeyCode::Right if !key.modifiers.contains(KeyModifiers::SHIFT) => self.next_tab(),
                    KeyCode::Left if !key.modifiers.contains(KeyModifiers::SHIFT) => self.previous_tab(),
                    KeyCode::Esc => self.quit(),

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

    fn next_tab(&mut self) {
        self.selected_tab = self.selected_tab.next();
    }

    fn previous_tab(&mut self) {
        self.selected_tab = self.selected_tab.previous();
    }

    fn quit(&mut self) {
        self.state = AppState::Quitting;
    }

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

    fn render_selected_tab(&mut self, area: Rect, buf: &mut Buffer) {
        let module = self.modules.get_mut(&self.selected_tab).expect("Tab must exist");
        let block = self.selected_tab.block().title(module.title());
        let inner = block.inner(area);

        block.render(area, buf);
        module.render(inner, buf);
    }
}

impl<S: Source + 'static> Widget for &mut App<S> {
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

impl<S: Source> Drop for App<S> {
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

fn render_title(area: Rect, buf: &mut Buffer) {
    "ODP EC Demo App".bold().render(area, buf);
}

fn render_footer(area: Rect, buf: &mut Buffer) {
    Line::raw("◄ ► to change tab | Press ESC to quit")
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
            Self::TabDebug => tailwind::GRAY,
            Self::TabBattery => tailwind::BLUE,
            Self::TabThermal => tailwind::EMERALD,
            Self::TabRTC => tailwind::INDIGO,
            Self::TabUCSI => tailwind::RED,
        }
    }
}
