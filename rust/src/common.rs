use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Padding, Widget},
};
use std::collections::VecDeque;

#[derive(Default)]
pub struct SampleBuf<T, const N: usize> {
    samples: VecDeque<T>,
}

impl<T, const N: usize> SampleBuf<T, N> {
    // Insert a sample into the buffer and evict the oldest if full
    pub fn insert(&mut self, sample: T) {
        self.samples.push_back(sample);
        if self.samples.len() > N {
            self.samples.pop_front();
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

impl<T: Into<f64> + Copy, const N: usize> SampleBuf<T, N> {
    // Converts the buffer into a format that ratatui can use for charts
    // Probably more efficent way than copying but buffer is small and only called once a second
    pub fn get(&self) -> Vec<(f64, f64)> {
        self.samples
            .iter()
            .enumerate()
            .map(|(i, &val)| (i as f64, val.into()))
            .collect()
    }
}

impl<T: Clone, const N: usize> SampleBuf<T, N> {
    // Some ratatui methods need a owned vec of data and unfortunately don't accept a ref
    pub fn as_vec(&self) -> Vec<T> {
        self.samples.clone().into()
    }
}

// Properties for rendering a graph
pub struct Graph {
    pub title: String,
    pub color: Color,
    pub samples: Vec<(f64, f64)>,

    pub x_axis: String,
    pub x_bounds: [f64; 2],
    pub x_labels: [Span<'static>; 3],

    pub y_axis: String,
    pub y_bounds: [f64; 2],
    pub y_labels: [Span<'static>; 3],
}

// Convert deciKelvin to degrees Celsius
pub const fn dk_to_c(dk: u32) -> f64 {
    (dk as f64 / 10.0) - 273.15
}

// Split an area in a direction with given percentages
pub fn area_split(area: Rect, direction: Direction, first: u16, second: u16) -> [Rect; 2] {
    Layout::default()
        .direction(direction)
        .constraints([Constraint::Percentage(first), Constraint::Percentage(second)])
        .areas(area)
}

// Splits an area in a direction with given constraints
pub fn area_split_constrained(area: Rect, direction: Direction, first: Constraint, second: Constraint) -> [Rect; 2] {
    Layout::default()
        .direction(direction)
        .constraints([first, second])
        .areas(area)
}

// Create a wrapping title block
pub fn title_block(title: &str, padding: u16, label_color: Color) -> Block<'_> {
    let title = Line::from(title);
    Block::new()
        .borders(Borders::ALL)
        .padding(Padding::vertical(padding))
        .title(title)
        .fg(label_color)
}

// Combines a title string with a visual status indicator character
pub fn title_str_with_status(title: &str, success: bool) -> String {
    let status = if success { "✅" } else { "❌" };
    format!("{title} {status}")
}

pub fn render_chart(area: Rect, buf: &mut Buffer, graph: Graph) {
    let samples = &graph.samples[..];
    let datasets = vec![
        Dataset::default()
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(graph.color))
            .graph_type(GraphType::Line)
            .data(samples),
    ];

    let chart = Chart::new(datasets)
        .block(Block::bordered().title(Line::from(graph.title).cyan().bold().centered()))
        .x_axis(
            Axis::default()
                .title(graph.x_axis)
                .style(Style::default().gray())
                .bounds(graph.x_bounds)
                .labels(graph.x_labels),
        )
        .y_axis(
            Axis::default()
                .title(graph.y_axis)
                .style(Style::default().gray())
                .bounds(graph.y_bounds)
                .labels(graph.y_labels),
        );

    chart.render(area, buf);
}

pub fn time_labels(t: usize, max_samples: usize) -> [Span<'static>; 3] {
    let (start, mid, end) = if t <= max_samples {
        (0, max_samples / 2, max_samples)
    } else {
        (t - max_samples, t - max_samples / 2, t)
    };
    [
        Span::styled(start.to_string(), Style::default().bold()),
        Span::styled(mid.to_string(), Style::default().bold()),
        Span::styled(end.to_string(), Style::default().bold()),
    ]
}
