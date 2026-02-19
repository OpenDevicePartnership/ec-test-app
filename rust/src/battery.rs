use crate::Source;
use crate::app::Module;
use crate::common;
use crate::widgets::battery;
use battery_service_messages::{
    BatteryState, BatterySwapCapability, BatteryTechnology, BixFixedStrings, BstReturn, PowerUnit,
};
use core::ffi::CStr;

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

fn str_from_bytes(bytes: &[u8]) -> String {
    CStr::from_bytes_until_nul(bytes)
        .ok()
        .and_then(|c| c.to_str().ok())
        .unwrap_or("Invalid")
        .to_owned()
}

fn charge_state_as_str(state: BatteryState) -> &'static str {
    if state.contains(BatteryState::DISCHARGING) {
        "Discharging"
    } else {
        "Charging"
    }
}

fn power_unit_as_capacity_str(power_unit: PowerUnit) -> &'static str {
    match power_unit {
        PowerUnit::MilliWatts => "mWh",
        PowerUnit::MilliAmps => "mAh",
    }
}

fn power_unit_as_rate_str(power_unit: PowerUnit) -> &'static str {
    match power_unit {
        PowerUnit::MilliWatts => "mW",
        PowerUnit::MilliAmps => "mA",
    }
}

fn bat_tech_as_str(bat_tech: BatteryTechnology) -> &'static str {
    match bat_tech {
        BatteryTechnology::Primary => "Primary",
        BatteryTechnology::Secondary => "Secondary",
    }
}

fn swap_cap_as_str(swap_cap: BatterySwapCapability) -> &'static str {
    match swap_cap {
        BatterySwapCapability::NonSwappable => "Non swappable",
        BatterySwapCapability::ColdSwappable => "Cold swappable",
        BatterySwapCapability::HotSwappable => "Hot swappable",
    }
}

struct BatteryTabState {
    btp: u32,
    btp_input: Input,
    bst_success: bool,
    bix_success: bool,
    btp_success: bool,
    samples: common::SampleBuf<u32, MAX_SAMPLES>,
}

impl Default for BatteryTabState {
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
    bst_data: BstReturn,
    bix_data: BixFixedStrings,
    state: BatteryTabState,
    t_sec: usize,
    t_min: usize,
    source: S,
}

impl<S: Source> Module for Battery<S> {
    fn title(&self) -> &'static str {
        "Battery Information"
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
        let update_graph = self.t_sec.is_multiple_of(60);

        self.t_sec += 1;
        if update_graph {
            self.state.samples.insert(self.bst_data.battery_remaining_capacity);
            self.t_min += 1;
        }
    }

    fn render(&self, area: Rect, buf: &mut Buffer) {
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
            y_axis: format!("Capacity ({})", power_unit_as_capacity_str(self.bix_data.power_unit)),
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
                power_unit_as_rate_str(power_unit).into(),
            ]),
            Row::new(vec![
                Text::raw("Design Capacity").add_modifier(Modifier::BOLD),
                format!(
                    "{} {}",
                    self.bix_data.design_capacity,
                    power_unit_as_capacity_str(power_unit)
                )
                .into(),
            ]),
            Row::new(vec![
                Text::raw("Last Full Capacity").add_modifier(Modifier::BOLD),
                format!(
                    "{} {}",
                    self.bix_data.last_full_charge_capacity,
                    power_unit_as_capacity_str(power_unit)
                )
                .into(),
            ]),
            Row::new(vec![
                Text::raw("Battery Technology").add_modifier(Modifier::BOLD),
                bat_tech_as_str(self.bix_data.battery_technology).into(),
            ]),
            Row::new(vec![
                Text::raw("Design Voltage").add_modifier(Modifier::BOLD),
                format!("{} mV", self.bix_data.design_voltage).into(),
            ]),
            Row::new(vec![
                Text::raw("Warning Capacity").add_modifier(Modifier::BOLD),
                format!(
                    "{} {}",
                    self.bix_data.design_cap_of_warning,
                    power_unit_as_capacity_str(power_unit)
                )
                .into(),
            ]),
            Row::new(vec![
                Text::raw("Low Capacity").add_modifier(Modifier::BOLD),
                format!(
                    "{} {}",
                    self.bix_data.design_cap_of_low,
                    power_unit_as_capacity_str(power_unit)
                )
                .into(),
            ]),
            Row::new(vec![
                Text::raw("Cycle Count").add_modifier(Modifier::BOLD),
                format!("{}", self.bix_data.cycle_count).into(),
            ]),
            Row::new(vec![
                Text::raw("Accuracy").add_modifier(Modifier::BOLD),
                format!("{}%", self.bix_data.measurement_accuracy as f64 / 1000.0).into(),
            ]),
            Row::new(vec![
                Text::raw("Max Sample Time").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.max_sampling_time).into(),
            ]),
            Row::new(vec![
                Text::raw("Mix Sample Time").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.min_sampling_time).into(),
            ]),
            Row::new(vec![
                Text::raw("Max Average Interval").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.max_averaging_interval).into(),
            ]),
            Row::new(vec![
                Text::raw("Min Average Interval").add_modifier(Modifier::BOLD),
                format!("{} ms", self.bix_data.min_averaging_interval).into(),
            ]),
            Row::new(vec![
                Text::raw("Capacity Granularity 1").add_modifier(Modifier::BOLD),
                format!(
                    "{} {}",
                    self.bix_data.battery_capacity_granularity_1,
                    power_unit_as_capacity_str(power_unit)
                )
                .into(),
            ]),
            Row::new(vec![
                Text::raw("Capacity Granularity 2").add_modifier(Modifier::BOLD),
                format!(
                    "{} {}",
                    self.bix_data.battery_capacity_granularity_2,
                    power_unit_as_capacity_str(power_unit)
                )
                .into(),
            ]),
            Row::new(vec![
                Text::raw("Model Number").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.bix_data.model_number).into(),
            ]),
            Row::new(vec![
                Text::raw("Serial Number").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.bix_data.serial_number).into(),
            ]),
            Row::new(vec![
                Text::raw("Battery Type").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.bix_data.battery_type).into(),
            ]),
            Row::new(vec![
                Text::raw("OEM Info").add_modifier(Modifier::BOLD),
                str_from_bytes(&self.bix_data.oem_info).into(),
            ]),
            Row::new(vec![
                Text::raw("Swapping Capability").add_modifier(Modifier::BOLD),
                swap_cap_as_str(self.bix_data.battery_swapping_capability).into(),
            ]),
        ]
    }

    fn render_bix(&self, area: Rect, buf: &mut Buffer) {
        let widths = [Constraint::Percentage(30), Constraint::Percentage(70)];
        let title = common::title_str_with_status("Battery Info", self.state.bix_success);
        let table = Table::new(self.create_info(), widths)
            .block(Block::bordered().title(title))
            .style(Style::new().white());
        Widget::render(table, area, buf);
    }

    fn create_status(&self) -> Vec<Line<'static>> {
        let power_unit = self.bix_data.power_unit;
        vec![
            Line::raw(format!(
                "State:               {}",
                charge_state_as_str(self.bst_data.battery_state)
            )),
            Line::raw(format!(
                "Present Rate:        {} {}",
                self.bst_data.battery_present_rate,
                power_unit_as_rate_str(power_unit)
            )),
            Line::raw(format!(
                "Remaining Capacity:  {} {}",
                self.bst_data.battery_remaining_capacity,
                power_unit_as_capacity_str(power_unit)
            )),
            Line::raw(format!(
                "Present Voltage:     {} mV",
                self.bst_data.battery_present_voltage
            )),
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
            power_unit_as_capacity_str(self.bix_data.power_unit)
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
        let mut state = battery::BatteryState::new(
            self.bst_data.battery_remaining_capacity,
            self.bst_data
                .battery_state
                .contains(battery_service_messages::BatteryState::CHARGING),
        );

        battery::Battery::default()
            .color_high(BATGAUGE_COLOR_HIGH)
            .color_warning(BATGAUGE_COLOR_MEDIUM)
            .color_low(BATGAUGE_COLOR_LOW)
            .design_capacity(self.bix_data.design_capacity)
            .warning_capacity(self.bix_data.design_cap_of_warning)
            .low_capacity(self.bix_data.design_cap_of_low)
            .render(area, buf, &mut state)
    }
}
