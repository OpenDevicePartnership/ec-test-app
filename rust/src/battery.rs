use std::borrow::Cow;

use crate::Source;
use crate::app::Module;
use crate::common;
use crate::widgets::battery;
use color_eyre::{Report, Result, eyre::eyre};

use ratatui::style::Modifier;
use ratatui::text::Text;
use ratatui::widgets::{Row, StatefulWidget, Table, Widget};
use ratatui::{
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEventKind},
    layout::{Constraint, Direction, Rect},
    style::{Color, Style, Stylize, palette::tailwind},
    text::{Line, Span},
    widgets::{Block, Paragraph},
};
use tui_input::{Input, backend::crossterm::EventHandler};

const BATGAUGE_COLOR_HIGH: Color = tailwind::GREEN.c500;
const BATGAUGE_COLOR_MEDIUM: Color = tailwind::YELLOW.c500;
const BATGAUGE_COLOR_LOW: Color = tailwind::RED.c500;
const LABEL_COLOR: Color = tailwind::SLATE.c200;
const MAX_SAMPLES: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ChargeState {
    #[default]
    Charging,
    Discharging,
}

impl TryFrom<u32> for ChargeState {
    type Error = Report;
    fn try_from(value: u32) -> Result<Self> {
        match value {
            1 => Ok(Self::Discharging),
            2 => Ok(Self::Charging),
            _ => Err(eyre!("Unknown charging state")),
        }
    }
}

impl ChargeState {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Charging => "Charging",
            Self::Discharging => "Discharging",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PowerUnit {
    #[default]
    Mw,
    Ma,
}

impl TryFrom<u32> for PowerUnit {
    type Error = Report;
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::Mw),
            1 => Ok(Self::Ma),
            _ => Err(eyre!("Unknown power unit")),
        }
    }
}

impl PowerUnit {
    fn as_capacity_str(&self) -> &'static str {
        match self {
            Self::Mw => "mWh",
            Self::Ma => "mAh",
        }
    }

    fn as_rate_str(&self) -> &'static str {
        match self {
            Self::Mw => "mW",
            Self::Ma => "mA",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum BatteryTechnology {
    #[default]
    Primary,
    Secondary,
}

impl TryFrom<u32> for BatteryTechnology {
    type Error = Report;
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::Primary),
            1 => Ok(Self::Secondary),
            _ => Err(eyre!("Unknown battery technology")),
        }
    }
}

impl BatteryTechnology {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Primary => "Primary",
            Self::Secondary => "Secondary",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum SwapCap {
    #[default]
    NonSwappable,
    ColdSwappable,
    HotSwappable,
}

impl TryFrom<u32> for SwapCap {
    type Error = Report;
    fn try_from(value: u32) -> Result<Self> {
        match value {
            0 => Ok(Self::NonSwappable),
            1 => Ok(Self::ColdSwappable),
            2 => Ok(Self::HotSwappable),
            _ => Err(eyre!("Unknown swapping capability")),
        }
    }
}

impl SwapCap {
    fn as_str(&self) -> &'static str {
        match self {
            Self::NonSwappable => "Non swappable",
            Self::ColdSwappable => "Cold swappable",
            Self::HotSwappable => "Hot swappable",
        }
    }
}

/// BST: ACPI Battery Status
#[derive(Default)]
pub struct BstData {
    pub state: ChargeState,
    pub rate: u32,
    pub capacity: u32,
    pub voltage: u32,
}

/// BIX: ACPI Battery Information eXtended
#[derive(Default)]
pub struct BixData {
    pub revision: u32,
    pub power_unit: PowerUnit, // 0 - mW, 1 - mA
    pub design_capacity: u32,
    pub last_full_capacity: u32,
    pub battery_technology: BatteryTechnology, // 0 - primary, 1 - secondary
    pub design_voltage: u32,
    pub warning_capacity: u32,
    pub low_capacity: u32,
    pub cycle_count: u32,
    pub accuracy: u32, // Thousands of a percent
    pub max_sample_time: u32,
    pub min_sample_time: u32, // Milliseconds
    pub max_average_interval: u32,
    pub min_average_interval: u32,
    pub capacity_gran1: u32,
    pub capacity_gran2: u32,
    pub model_number: String,
    pub serial_number: String,
    pub battery_type: String,
    pub oem_info: String,
    pub swap_cap: SwapCap,
}

struct BatteryState {
    btp: u32,
    btp_input: Input,
    bst_success: bool,
    bix_success: bool,
    btp_success: bool,
    samples: common::SampleBuf<u32, MAX_SAMPLES>,
}

impl Default for BatteryState {
    fn default() -> Self {
        Self {
            btp: 0,
            btp_input: Input::default(),
            bst_success: false,
            bix_success: false,
            btp_success: true,
            samples: common::SampleBuf::default(),
        }
    }
}

pub struct Battery<S: Source> {
    bst_data: BstData,
    bix_data: BixData,
    state: BatteryState,
    t_sec: usize,
    t_min: usize,
    source: S,
}

impl<S: Source> Module for Battery<S> {
    fn title(&self) -> Cow<'static, str> {
        "Battery Information".into()
    }

    fn update(&mut self) {
        if let Ok(bst_data) = self.source.get_bst() {
            self.bst_data = bst_data;
            self.state.bst_success = true;
        } else {
            self.state.bst_success = false;
        }

        // In mock demo, update graph every second, but real-life update every minute
        #[cfg(feature = "mock")]
        let update_graph = true;
        #[cfg(not(feature = "mock"))]
        let update_graph = (self.t_sec % 60) == 0;

        self.t_sec += 1;
        if update_graph {
            self.state.samples.insert(self.bst_data.capacity);
            self.t_min += 1;
        }
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let [info_area, charge_area] = common::area_split(area, Direction::Horizontal, 80, 20);
        self.render_info(info_area, buf);
        self.render_battery(charge_area, buf);
    }

    fn handle_event(&mut self, evt: &Event) {
        if let Event::Key(key) = evt
            && key.code == KeyCode::Enter
            && key.kind == KeyEventKind::Press
        {
            if let Ok(btp) = self.state.btp_input.value_and_reset().parse() {
                if self.source.set_btp(btp).is_ok() {
                    self.state.btp = btp;
                    self.state.btp_success = true;
                } else {
                    self.state.btp_success = false;
                }
            }
        } else {
            let _ = self.state.btp_input.handle_event(evt);
        }
    }
}

impl<S: Source> Battery<S> {
    pub fn new(source: S) -> Self {
        let mut inst = Self {
            bst_data: Default::default(),
            bix_data: Default::default(),
            state: Default::default(),
            t_sec: Default::default(),
            t_min: Default::default(),
            source,
        };

        // This shouldn't change because BIX info is static so just read once
        if let Ok(bix_data) = inst.source.get_bix() {
            inst.bix_data = bix_data;
            inst.state.bix_success = true;
        } else {
            inst.state.bix_success = false;
        }

        inst.update();
        inst
    }

    fn render_info(&self, area: Rect, buf: &mut Buffer) {
        let [bix_area, status_area] = common::area_split(area, Direction::Horizontal, 50, 50);
        let [bst_area, btp_area] = common::area_split(status_area, Direction::Vertical, 70, 30);
        let [bst_chart_area, bst_info_area] = common::area_split(bst_area, Direction::Vertical, 65, 35);

        self.render_bix(bix_area, buf);
        self.render_bst(bst_info_area, buf);
        self.render_bst_chart(bst_chart_area, buf);
        self.render_btp(btp_area, buf);
    }

    fn render_bst_chart(&self, area: Rect, buf: &mut Buffer) {
        let y_labels = [
            "0".bold(),
            Span::styled(
                format!("{}", self.bix_data.design_capacity / 2),
                Style::default().bold(),
            ),
            Span::styled(format!("{}", self.bix_data.design_capacity), Style::default().bold()),
        ];
        let graph = common::Graph {
            title: "Capacity vs Time".to_string(),
            color: Color::Red,
            samples: self.state.samples.get(),
            x_axis: "Time (m)".to_string(),
            x_bounds: [0.0, 60.0],
            x_labels: common::time_labels(self.t_min, MAX_SAMPLES),
            y_axis: format!("Capacity ({})", self.bix_data.power_unit.as_capacity_str()),
            y_bounds: [0.0, self.bix_data.design_capacity as f64],
            y_labels,
        };
        common::render_chart(area, buf, graph);
    }

    fn create_info(&self) -> Vec<Row<'static>> {
        let power_unit = self.bix_data.power_unit;

        vec![
            Row::new(vec![
                Text::styled("Revision", Style::default().add_modifier(Modifier::BOLD)),
                format!("{}", self.bix_data.revision).into(),
            ]),
            Row::new(vec![
                Text::raw("Power Unit").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.power_unit.as_rate_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Design Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.bix_data.design_capacity, power_unit.as_capacity_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Last Full Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.bix_data.last_full_capacity, power_unit.as_capacity_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Battery Technology").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.battery_technology.as_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Design Voltage").add_modifier(Modifier::BOLD),
                format!("{} mV", self.bix_data.design_voltage).into(),
            ]),
            Row::new(vec![
                Text::raw("Warning Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.bix_data.warning_capacity, power_unit.as_capacity_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Low Capacity").add_modifier(Modifier::BOLD),
                format!("{} {}", self.bix_data.low_capacity, power_unit.as_capacity_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Cycle Count").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.cycle_count).into(),
            ]),
            Row::new(vec![
                Text::raw("Accuracy").add_modifier(Modifier::BOLD),
                format!("{}%", self.bix_data.accuracy as f64 / 1000.0).into(),
            ]),
            Row::new(vec![
                Text::raw("Max Sample Time").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.max_sample_time).into(),
            ]),
            Row::new(vec![
                Text::raw("Mix Sample Time").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.min_sample_time).into(),
            ]),
            Row::new(vec![
                Text::raw("Max Average Interval").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.max_average_interval).into(),
            ]),
            Row::new(vec![
                Text::raw("Min Average Interval").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.min_average_interval).into(),
            ]),
            Row::new(vec![
                Text::raw("Capacity Granularity 1").add_modifier(Modifier::BOLD),
                format!("{} {}", self.bix_data.capacity_gran1, power_unit.as_capacity_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Capacity Granularity 2").add_modifier(Modifier::BOLD),
                format!("{} {}", self.bix_data.capacity_gran2, power_unit.as_capacity_str()).into(),
            ]),
            Row::new(vec![
                Text::raw("Model Number").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.model_number).into(),
            ]),
            Row::new(vec![
                Text::raw("Serial Number").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.serial_number).into(),
            ]),
            Row::new(vec![
                Text::raw("Battery Type").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.battery_type).into(),
            ]),
            Row::new(vec![
                Text::raw("OEM Info").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.oem_info).into(),
            ]),
            Row::new(vec![
                Text::raw("Swapping Capability").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.swap_cap.as_str()).into(),
            ]),
        ]
    }

    fn render_bix(&self, area: Rect, buf: &mut Buffer) {
        let widths = [Constraint::Percentage(30), Constraint::Percentage(70)];
        let table = Table::new(self.create_info(), widths)
            .block(Block::bordered().title("Battery Info"))
            .style(Style::new().white());
        Widget::render(table, area, buf);
    }

    fn create_status(&self) -> Vec<Line<'static>> {
        let power_unit = self.bix_data.power_unit;
        vec![
            Line::raw(format!("State:               {}", self.bst_data.state.as_str())),
            Line::raw(format!(
                "Present Rate:        {} {}",
                self.bst_data.rate,
                power_unit.as_rate_str()
            )),
            Line::raw(format!(
                "Remaining Capacity:  {} {}",
                self.bst_data.capacity,
                power_unit.as_capacity_str()
            )),
            Line::raw(format!("Present Voltage:     {} mV", self.bst_data.voltage)),
        ]
    }

    fn render_bst(&self, area: Rect, buf: &mut Buffer) {
        let title = common::title_str_with_status("Battery Status", self.state.bst_success);
        let title = common::title_block(&title, 0, LABEL_COLOR);
        Paragraph::new(self.create_status()).block(title).render(area, buf);
    }

    fn create_trippoint(&self) -> Vec<Line<'static>> {
        vec![Line::raw(format!(
            "Current: {} {}",
            self.state.btp,
            self.bix_data.power_unit.as_capacity_str(),
        ))]
    }

    fn render_btp(&self, area: Rect, buf: &mut Buffer) {
        let title_str = common::title_str_with_status("Trippoint", self.state.btp_success);
        let title = common::title_block(&title_str, 0, LABEL_COLOR);
        let inner = title.inner(area);
        title.render(area, buf);

        let [current_area, input_area] =
            common::area_split_constrained(inner, Direction::Vertical, Constraint::Min(0), Constraint::Max(3));

        Paragraph::new(self.create_trippoint()).render(current_area, buf);
        self.render_btp_input(input_area, buf);
    }

    fn render_btp_input(&self, area: Rect, buf: &mut Buffer) {
        let width = area.width.max(3) - 3;
        let scroll = self.state.btp_input.visual_scroll(width as usize);

        let input = Paragraph::new(self.state.btp_input.value())
            .style(Style::default())
            .scroll((0, scroll as u16))
            .block(Block::bordered().title("Set Trippoint <ENTER>"));
        input.render(area, buf);
    }

    fn render_battery(&self, area: Rect, buf: &mut Buffer) {
        let mut state =
            battery::BatteryState::new(self.bst_data.capacity, self.bst_data.state == ChargeState::Charging);

        battery::Battery::default()
            .color_high(BATGAUGE_COLOR_HIGH)
            .color_warning(BATGAUGE_COLOR_MEDIUM)
            .color_low(BATGAUGE_COLOR_LOW)
            .design_capacity(self.bix_data.design_capacity)
            .warning_capacity(self.bix_data.warning_capacity)
            .low_capacity(self.bix_data.low_capacity)
            .render(area, buf, &mut state)
    }
}
