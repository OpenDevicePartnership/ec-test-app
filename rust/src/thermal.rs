use std::borrow::Cow;

use crate::app::Module;
use crate::common;
use crate::{Source, Threshold};
use color_eyre::Result;
use ratatui::layout::Constraint;
use ratatui::{
    buffer::Buffer,
    crossterm::event::{Event, KeyCode, KeyEventKind},
    layout::{Direction, Rect},
    style::{Color, Style, Stylize, palette::tailwind},
    text::{Line, Span},
    widgets::{Block, Gauge, Paragraph, Widget},
};
use tui_input::{Input, backend::crossterm::EventHandler};

const LABEL_COLOR: Color = tailwind::SLATE.c200;
const MAX_SAMPLES: usize = 60;

fn get_sensor_tmp<S: Source>(source: &S) -> Result<f64> {
    source.get_temperature()
}

// Always return mock data for thresholds until sensor GET/SET VAR and GET/SET THRS supported
fn get_sensor_thresholds<S: Source>(_source: &S) -> Result<SensorThresholds> {
    Ok(SensorThresholds {
        _warn_low: 13.0,
        warn_high: 35.0,
        prochot: 40.0,
        critical: 45.0,
    })
}

fn get_fan_rpm<S: Source>(source: &S) -> Result<f64> {
    source.get_rpm()
}

fn set_fan_rpm<S: Source>(source: &S, rpm: f64) -> Result<()> {
    source.set_rpm(rpm)
}

fn get_fan_bounds<S: Source>(source: &S) -> Result<FanRpmBounds> {
    let min = source.get_min_rpm()?;
    let max = source.get_max_rpm()?;

    Ok(FanRpmBounds { min, max })
}

fn get_fan_levels<S: Source>(source: &S) -> Result<FanStateLevels> {
    let on = source.get_threshold(Threshold::On)?;
    let ramping = source.get_threshold(Threshold::Ramping)?;
    let max = source.get_threshold(Threshold::Max)?;

    Ok(FanStateLevels { on, ramping, max })
}

#[derive(Default)]
struct SensorThresholds {
    _warn_low: f64,
    warn_high: f64,
    prochot: f64,
    critical: f64,
}

#[derive(Default)]
struct SensorState {
    skin_temp: f64,
    temp_success: bool,
    thresholds: SensorThresholds,
    thresholds_success: bool,
    samples: common::SampleBuf<f64, MAX_SAMPLES>,
}

impl SensorState {
    fn update<S: Source>(&mut self, source: &S) {
        if let Ok(temp) = get_sensor_tmp(source) {
            self.skin_temp = temp;
            self.samples.insert(temp);
            self.temp_success = true;
        } else {
            self.temp_success = false;
        }

        if let Ok(thresholds) = get_sensor_thresholds(source) {
            self.thresholds = thresholds;
            self.thresholds_success = true;
        } else {
            self.thresholds_success = false;
        }
    }
}

#[derive(Default)]
struct FanRpmBounds {
    min: f64,
    max: f64,
}

#[derive(Default)]
struct FanStateLevels {
    on: f64,
    ramping: f64,
    max: f64,
}

#[derive(Default)]
struct FanState {
    rpm: f64,
    rpm_success: bool,
    rpm_bounds: FanRpmBounds,
    bounds_success: bool,
    state_levels: FanStateLevels,
    levels_success: bool,
    samples: common::SampleBuf<u32, MAX_SAMPLES>,
}

impl FanState {
    fn update<S: Source>(&mut self, source: &S) {
        if let Ok(rpm) = get_fan_rpm(source) {
            self.rpm = rpm;
            self.samples.insert(rpm as u32);
            self.rpm_success = true;
        } else {
            self.rpm_success = false;
        }

        if let Ok(rpm_bounds) = get_fan_bounds(source) {
            self.rpm_bounds = rpm_bounds;
            self.bounds_success = true;
        } else {
            self.bounds_success = false;
        }

        if let Ok(state_levels) = get_fan_levels(source) {
            self.state_levels = state_levels;
            self.levels_success = true;
        } else {
            self.levels_success = false;
        }
    }
}

pub struct Thermal<S: Source> {
    rpm_input: Input,
    sensor: SensorState,
    fan: FanState,
    t: usize,
    source: S,
}

impl<S: Source> Module for Thermal<S> {
    fn title(&self) -> Cow<'static, str> {
        "Thermal Information".into()
    }

    fn update(&mut self) {
        self.sensor.update(&self.source);
        self.fan.update(&self.source);
        self.t += 1;
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let [sensor_area, fan_area] = common::area_split(area, Direction::Horizontal, 50, 50);
        self.render_sensor(sensor_area, buf);
        self.render_fan(fan_area, buf);
    }

    fn handle_event(&mut self, evt: &Event) {
        if let Event::Key(key) = evt
            && key.code == KeyCode::Enter
            && key.kind == KeyEventKind::Press
        {
            if let Ok(rpm) = self.rpm_input.value_and_reset().parse() {
                let _ = set_fan_rpm(&self.source, rpm);
            }
        } else {
            let _ = self.rpm_input.handle_event(evt);
        }
    }
}

impl<S: Source> Thermal<S> {
    pub fn new(source: S) -> Self {
        let mut inst = Self {
            rpm_input: Default::default(),
            sensor: Default::default(),
            fan: Default::default(),
            t: Default::default(),
            source,
        };

        inst.update();
        inst
    }

    fn render_sensor(&self, area: Rect, buf: &mut Buffer) {
        let [chart_area, widget_area] = common::area_split(area, Direction::Vertical, 70, 30);
        let [stats_area, thresholds_area] = common::area_split(widget_area, Direction::Horizontal, 50, 50);
        self.render_sensor_chart(chart_area, buf);
        self.render_sensor_stats(stats_area, buf);
        self.render_sensor_thresholds(thresholds_area, buf);
    }

    fn render_sensor_chart(&self, area: Rect, buf: &mut Buffer) {
        let y_labels = [
            "0.0".bold(),
            Span::styled(
                format!("{:.1}", (self.sensor.thresholds.critical + 5.0) / 2.0),
                Style::default().bold(),
            ),
            Span::styled(
                format!("{:.1}", self.sensor.thresholds.critical + 5.0),
                Style::default().bold(),
            ),
        ];
        let graph = common::Graph {
            title: "Temperature vs Time".to_string(),
            color: Color::Red,
            samples: self.sensor.samples.get(),
            x_axis: "Time (s)".to_string(),
            x_bounds: [0.0, 60.0],
            x_labels: common::time_labels(self.t, MAX_SAMPLES),
            y_axis: "Temperature (°C)".to_string(),
            y_bounds: [0.0, self.sensor.thresholds.critical + 5.0],
            y_labels,
        };
        common::render_chart(area, buf, graph);
    }

    fn create_sensor_stats(&self) -> Vec<Line<'static>> {
        vec![Line::raw(format!("Skin temp: {:.2} °C", self.sensor.skin_temp))]
    }

    fn render_sensor_stats(&self, area: Rect, buf: &mut Buffer) {
        let title_str = common::title_str_with_status("Live Temperature", self.sensor.temp_success);
        let stats_title = common::title_block(&title_str, 1, LABEL_COLOR);
        let inner = stats_title.inner(area);
        stats_title.render(area, buf);
        let [temp_area, gauge_area] = common::area_split(inner, Direction::Vertical, 50, 50);

        let gauge_color = if self.sensor.skin_temp < self.sensor.thresholds.warn_high {
            tailwind::GREEN.c700
        } else if self.sensor.skin_temp < self.sensor.thresholds.prochot {
            tailwind::YELLOW.c700
        } else if self.sensor.skin_temp < self.sensor.thresholds.critical {
            tailwind::ORANGE.c700
        } else {
            tailwind::RED.c700
        };
        let gauge_percent = (((self.sensor.skin_temp / self.sensor.thresholds.critical) * 100.0) as u16).clamp(0, 100);
        Paragraph::new(self.create_sensor_stats()).render(temp_area, buf);
        Gauge::default()
            .gauge_style(gauge_color)
            .percent(gauge_percent)
            .render(gauge_area, buf);
    }

    fn create_sensor_thresholds(&self) -> Vec<Line<'static>> {
        vec![
            Line::raw(format!("Warn:     {} °C", self.sensor.thresholds.warn_high.round())),
            Line::raw(format!("Prochot:  {} °C", self.sensor.thresholds.prochot.round())),
            Line::raw(format!("Critical: {} °C", self.sensor.thresholds.critical.round())),
        ]
    }

    fn render_sensor_thresholds(&self, area: Rect, buf: &mut Buffer) {
        let title_str = common::title_str_with_status("Thresholds", self.sensor.thresholds_success);
        let title = common::title_block(&title_str, 1, LABEL_COLOR);
        Paragraph::new(self.create_sensor_thresholds())
            .block(title)
            .render(area, buf);
    }

    fn render_fan(&self, area: Rect, buf: &mut Buffer) {
        let [chart_area, widget_area] = common::area_split(area, Direction::Vertical, 70, 30);
        let [stats_area, levels_area] = common::area_split(widget_area, Direction::Horizontal, 50, 50);
        self.render_fan_chart(chart_area, buf);
        self.render_fan_stats(stats_area, buf);
        self.render_fan_levels(levels_area, buf);
    }

    fn render_fan_chart(&self, area: Rect, buf: &mut Buffer) {
        let y_labels = [
            "0.0".bold(),
            Span::styled((self.fan.rpm_bounds.max / 2.0).to_string(), Style::default().bold()),
            Span::styled(self.fan.rpm_bounds.max.to_string(), Style::default().bold()),
        ];
        let graph = common::Graph {
            title: "Fan RPM vs Time".to_string(),
            color: Color::Blue,
            samples: self.fan.samples.get(),
            x_axis: "Time (s)".to_string(),
            x_bounds: [0.0, 60.0],
            x_labels: common::time_labels(self.t, MAX_SAMPLES),
            y_axis: "RPM".to_string(),
            y_bounds: [0.0, self.fan.rpm_bounds.max],
            y_labels,
        };
        common::render_chart(area, buf, graph);
    }

    fn create_fan_stats(&self) -> Vec<Line<'static>> {
        vec![Line::raw(format!(
            "RPM: {} ({}, {})",
            self.fan.rpm.round(),
            self.fan.rpm_bounds.min,
            self.fan.rpm_bounds.max
        ))]
    }

    fn render_fan_stats(&self, area: Rect, buf: &mut Buffer) {
        let title_str = common::title_str_with_status("Live Fan RPM", self.fan.rpm_success && self.fan.bounds_success);
        let title = common::title_block(&title_str, 0, LABEL_COLOR);
        let inner = title.inner(area);
        title.render(area, buf);

        let [rpm_area, input_area] =
            common::area_split_constrained(inner, Direction::Vertical, Constraint::Min(0), Constraint::Max(3));

        Paragraph::new(self.create_fan_stats()).render(rpm_area, buf);
        self.render_fan_rpm_input(input_area, buf);
    }

    fn create_fan_levels(&self) -> Vec<Line<'static>> {
        vec![
            Line::raw(format!("On:      {} °C", self.fan.state_levels.on.round())),
            Line::raw(format!("Ramping: {} °C", self.fan.state_levels.ramping.round())),
            Line::raw(format!("Max:     {} °C", self.fan.state_levels.max.round())),
        ]
    }

    fn render_fan_levels(&self, area: Rect, buf: &mut Buffer) {
        let title_str = common::title_str_with_status("Fan State Levels", self.fan.levels_success);
        let title = common::title_block(&title_str, 1, LABEL_COLOR);
        Paragraph::new(self.create_fan_levels()).block(title).render(area, buf);
    }

    fn render_fan_rpm_input(&self, area: Rect, buf: &mut Buffer) {
        let width = area.width.max(3) - 3;
        let scroll = self.rpm_input.visual_scroll(width as usize);

        let input = Paragraph::new(self.rpm_input.value())
            .style(Style::default())
            .scroll((0, scroll as u16))
            .block(Block::bordered().title("Set Fan RPM <ENTER>"));
        input.render(area, buf);
    }
}
