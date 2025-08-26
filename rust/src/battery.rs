use crate::Source;
use crate::app::Module;
use crate::common;
use color_eyre::{Report, Result, eyre::eyre};

use ratatui::{
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEventKind},
    layout::{Direction, Rect},
    style::{Color, Style, Stylize, palette::tailwind},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, BorderType, Borders, Paragraph, Widget},
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

#[derive(Default)]
pub struct Battery<S: Source> {
    bst_data: BstData,
    bix_data: BixData,
    state: BatteryState,
    t_sec: usize,
    t_min: usize,
    source: S,
}

impl<S: Source> Module for Battery<S> {
    fn title(&self) -> String {
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

    fn create_info(&self) -> Vec<Line<'static>> {
        let power_unit = self.bix_data.power_unit;
        vec![
            Line::raw(format!("Revision:               {}", self.bix_data.revision)),
            Line::raw(format!(
                "Power Unit:             {}",
                self.bix_data.power_unit.as_rate_str()
            )),
            Line::raw(format!(
                "Design Capacity:        {} {}",
                self.bix_data.design_capacity,
                power_unit.as_capacity_str()
            )),
            Line::raw(format!(
                "Last Full Capacity:     {} {}",
                self.bix_data.last_full_capacity,
                power_unit.as_capacity_str()
            )),
            Line::raw(format!(
                "Battery Technology:     {}",
                self.bix_data.battery_technology.as_str()
            )),
            Line::raw(format!("Design Voltage:         {} mV", self.bix_data.design_voltage)),
            Line::raw(format!(
                "Warning Capacity:       {} {}",
                self.bix_data.warning_capacity,
                power_unit.as_capacity_str()
            )),
            Line::raw(format!(
                "Low Capacity:           {} {}",
                self.bix_data.low_capacity,
                power_unit.as_capacity_str()
            )),
            Line::raw(format!("Cycle Count:            {}", self.bix_data.cycle_count)),
            Line::raw(format!(
                "Accuracy:               {}%",
                self.bix_data.accuracy as f64 / 1000.0
            )),
            Line::raw(format!("Max Sample Time:        {} ms", self.bix_data.max_sample_time)),
            Line::raw(format!("Min Sample Time:        {} ms", self.bix_data.min_sample_time)),
            Line::raw(format!(
                "Max Average Interval:   {} ms",
                self.bix_data.max_average_interval
            )),
            Line::raw(format!(
                "Min Average Interval:   {} ms",
                self.bix_data.min_average_interval
            )),
            Line::raw(format!(
                "Capacity Granularity 1: {} {}",
                self.bix_data.capacity_gran1,
                power_unit.as_capacity_str()
            )),
            Line::raw(format!(
                "Capacity Granularity 2: {} {}",
                self.bix_data.capacity_gran2,
                power_unit.as_capacity_str()
            )),
            Line::raw(format!("Model Number:           {}", self.bix_data.model_number)),
            Line::raw(format!("Serial Number:          {}", self.bix_data.serial_number)),
            Line::raw(format!("Battery Type:           {}", self.bix_data.battery_type)),
            Line::raw(format!("OEM Info:               {}", self.bix_data.oem_info)),
            Line::raw(format!("Swapping Capability:    {}", self.bix_data.swap_cap.as_str())),
        ]
    }

    fn render_bix(&self, area: Rect, buf: &mut Buffer) {
        let title = common::title_str_with_status("Battery Info", self.state.bix_success);
        let title = common::title_block(&title, 1, LABEL_COLOR);
        Paragraph::new(self.create_info()).block(title).render(area, buf);
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
            self.bix_data.power_unit.as_capacity_str()
        ))]
    }

    fn render_btp(&self, area: Rect, buf: &mut Buffer) {
        let title_str = common::title_str_with_status("Trippoint", self.state.btp_success);
        let title = common::title_block(&title_str, 0, LABEL_COLOR);
        let inner = title.inner(area);
        title.render(area, buf);

        let [current_area, input_area] = common::area_split(inner, Direction::Vertical, 30, 70);

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
        let bat_percent = (self.bst_data.capacity * 100)
            .checked_div(self.bix_data.design_capacity)
            .unwrap_or(0)
            .clamp(0, 100);

        let [tip_area, battery_area] = common::area_split(area, Direction::Vertical, 10, 90);
        let bar = Bar::default()
            .value(bat_percent as u64)
            .text_value(format!("{bat_percent}%"));
        let color = if self.bst_data.capacity < self.bix_data.low_capacity {
            BATGAUGE_COLOR_LOW
        } else if self.bst_data.capacity < self.bix_data.warning_capacity {
            BATGAUGE_COLOR_MEDIUM
        } else {
            BATGAUGE_COLOR_HIGH
        };

        BarChart::default()
            .data(BarGroup::default().bars(&[bar]))
            .max(100)
            .bar_gap(0)
            .bar_style(Style::default().fg(color))
            .block(Block::default().borders(Borders::ALL).border_type(BorderType::Double))
            .bar_width(battery_area.width - 2)
            .render(battery_area, buf);

        let width = tip_area.width / 3;
        let x = tip_area.x + (tip_area.width - width) / 2;
        let tip_area = Rect {
            x,
            y: tip_area.y,
            width,
            height: tip_area.height,
        };
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_type(BorderType::Double)
            .render(tip_area, buf);

        if self.bst_data.state == ChargeState::Charging {
            render_bolt(battery_area, buf);
        }
    }
}

fn render_bolt(area: Rect, buf: &mut Buffer) {
    // Bolt outline
    const BOLT: [(f64, f64); 7] = [
        (0.60, 0.05),
        (0.42, 0.40),
        (0.64, 0.40),
        (0.26, 0.95),
        (0.50, 0.55),
        (0.32, 0.55),
        (0.60, 0.05),
    ];
    let area = Rect {
        x: area.x + area.width / 15,
        y: area.y + area.height / 4,
        width: area.width,
        height: area.height / 2,
    };

    // fill the bolt with dense points using braille marker (2x4 subcells per cell)
    ratatui::widgets::canvas::Canvas::default()
        .x_bounds([0.0, 1.0])
        .y_bounds([0.0, 1.0])
        .marker(ratatui::symbols::Marker::Braille)
        .paint(|ctx| {
            let mut pts: Vec<(f64, f64)> = Vec::new();

            // sampling density (increase if you want smoother)
            const SX: usize = 160; // sub-samples across X
            const SY: usize = 320; // sub-samples across Y

            for iy in 0..SY {
                let y = (iy as f64 + 0.5) / SY as f64;
                // find polygon-edge intersections with this scanline
                let mut xs: Vec<f64> = Vec::new();
                for i in 0..BOLT.len() - 1 {
                    let (x1, y1) = BOLT[i];
                    let (x2, y2) = BOLT[i + 1];
                    if (y1 > y) != (y2 > y) && (y2 - y1).abs() > f64::EPSILON {
                        let t = (y - y1) / (y2 - y1);
                        xs.push(x1 + t * (x2 - x1));
                    }
                }
                xs.sort_by(|a, b| a.partial_cmp(b).unwrap());

                // fill between pairs of intersections with sub-sampled points
                for pair in xs.chunks(2) {
                    if pair.len() < 2 {
                        continue;
                    }
                    let (x0, x1) = (pair[0], pair[1]);
                    let steps = ((x1 - x0) * SX as f64).max(1.0).ceil() as usize;
                    for s in 0..steps {
                        let x = x0 + (s as f64 + 0.5) / SX as f64;
                        pts.push((x, y));
                    }
                }
            }

            ctx.draw(&ratatui::widgets::canvas::Points {
                coords: pts.as_slice(),
                color: Color::Yellow,
            });
        })
        .render(area, buf);
}
