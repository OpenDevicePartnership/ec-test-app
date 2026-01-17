use crate::common;
use crate::widgets::bolt::Bolt;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    prelude::Direction,
    style::{Color, Style},
    widgets::{Bar, BarChart, BarGroup, Block, BorderType, Borders, StatefulWidget, Widget},
};

pub struct BatteryState {
    current_capacity: u32,
    is_charging: bool,
}

impl BatteryState {
    pub fn new(current_capacity: u32, is_charging: bool) -> Self {
        Self {
            current_capacity,
            is_charging,
        }
    }
}

impl Default for BatteryState {
    fn default() -> Self {
        Self::new(0, false)
    }
}

pub struct Battery {
    color_high: Color,
    color_warning: Color,
    color_low: Color,
    design_capacity: u32,
    warning_capacity: u32,
    low_capacity: u32,
}

impl Default for Battery {
    fn default() -> Self {
        Self::new(Color::Black, Color::Black, Color::Black, 0, 0, 0)
    }
}

impl Battery {
    pub fn new(
        color_high: Color,
        color_warning: Color,
        color_low: Color,
        design_capacity: u32,
        warning_capacity: u32,
        low_capacity: u32,
    ) -> Self {
        Self {
            color_high,
            color_warning,
            color_low,
            design_capacity,
            warning_capacity,
            low_capacity,
        }
    }

    pub fn color_high(self, color: Color) -> Self {
        Self {
            color_high: color,
            ..self
        }
    }

    pub fn color_warning(self, color: Color) -> Self {
        Self {
            color_warning: color,
            ..self
        }
    }

    pub fn color_low(self, color: Color) -> Self {
        Self {
            color_low: color,
            ..self
        }
    }

    pub fn design_capacity(self, capacity: u32) -> Self {
        Self {
            design_capacity: capacity,
            ..self
        }
    }

    pub fn warning_capacity(self, capacity: u32) -> Self {
        Self {
            warning_capacity: capacity,
            ..self
        }
    }

    pub fn low_capacity(self, capacity: u32) -> Self {
        Self {
            low_capacity: capacity,
            ..self
        }
    }
}

impl StatefulWidget for Battery {
    type State = BatteryState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let bat_percent = (state.current_capacity * 100)
            .checked_div(self.design_capacity)
            .unwrap_or(0)
            .clamp(0, 100);

        let [tip_area, battery_area] = common::area_split(area, Direction::Vertical, 10, 90);
        let bar = Bar::default()
            .value(bat_percent as u64)
            .text_value(format!("{bat_percent}%"));

        let color = if state.current_capacity < self.low_capacity {
            self.color_low
        } else if state.current_capacity < self.warning_capacity {
            self.color_warning
        } else {
            self.color_high
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

        if state.is_charging {
            Bolt.render(battery_area, buf)
        }
    }
}
