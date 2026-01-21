use crate::common;
use crossterm::event::Event;
use embedded_mcu_hal::time::Datetime;
use ratatui::{
    prelude::*,
    style::{Color, palette::tailwind},
    widgets::Paragraph,
};
use time_alarm_service_messages::{
    AcpiDaylightSavingsTimeStatus, AcpiTimeZone, AcpiTimerId, AcpiTimestamp, AlarmExpiredWakePolicy, AlarmTimerSeconds,
    TimeAlarmDeviceCapabilities, TimerStatus,
};

use crate::app::Module;
use crate::{RtcSource, Source};

const LABEL_COLOR: Color = tailwind::SLATE.c200;

mod rtc_timer {
    use super::*;
    pub struct RtcTimer {
        timer_id: AcpiTimerId,

        value: AlarmTimerSeconds,
        wake_policy: AlarmExpiredWakePolicy,
        timer_status: TimerStatus,
    }

    impl RtcTimer {
        pub fn update(&mut self, source: &impl RtcSource) -> color_eyre::Result<()> {
            self.value = source.get_timer_value(self.timer_id)?;
            self.wake_policy = source.get_expired_timer_wake_policy(self.timer_id)?;
            self.timer_status = source.get_wake_status(self.timer_id)?;
            Ok(())
        }

        pub fn new(timer_id: AcpiTimerId) -> Self {
            Self {
                timer_id,
                value: AlarmTimerSeconds::DISABLED,
                wake_policy: AlarmExpiredWakePolicy::INSTANTLY,
                timer_status: TimerStatus::default(),
            }
        }

        pub fn render(&self, title: &str, area: Rect, buf: &mut Buffer) {
            Paragraph::new(vec![
                Line::raw(format!(
                    "Time remaining:       {}",
                    match self.value {
                        AlarmTimerSeconds::DISABLED => "Timer not set".to_string(),
                        seconds => format!("{} seconds", seconds.0),
                    }
                )),
                Line::raw(format!(
                    "Wake policy:          {}",
                    match self.wake_policy {
                        AlarmExpiredWakePolicy::NEVER => "never".to_string(),
                        AlarmExpiredWakePolicy::INSTANTLY => "instantly".to_string(),
                        wake_policy => format!("after {} seconds", wake_policy.0),
                    }
                )),
                Line::raw(format!("Timer expired:        {}", self.timer_status.timer_expired())),
                Line::raw(format!(
                    "Timer triggered wake: {}",
                    self.timer_status.timer_triggered_wake()
                )),
            ])
            .block(common::title_block(title, 0, LABEL_COLOR))
            .render(area, buf);
        }
    }
}

use rtc_timer::RtcTimer;

pub struct Rtc<S: Source> {
    source: S,
    timers: [RtcTimer; 2],

    capabilities: TimeAlarmDeviceCapabilities,
    timestamp: AcpiTimestamp,
}

impl<S: Source> Module for Rtc<S> {
    fn title(&self) -> &'static str {
        "RTC Information"
    }

    fn update(&mut self) {
        self.timestamp = self.source.get_real_time().unwrap();
        for timer in &mut self.timers {
            timer.update(&self.source).unwrap();
        }
    }

    fn handle_event(&mut self, _evt: &Event) {}

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let title = common::title_block("Real-time Clock", 0, LABEL_COLOR);

        let [general_area, timers_area] = common::area_split(area, Direction::Vertical, 70, 30);
        let [ac_area, dc_area] = common::area_split(timers_area, Direction::Horizontal, 50, 50);
        let general_messages = vec![
            format!("Time:      {}", format_time(self.timestamp.datetime)),
            format!("Time Zone: {}", format_time_zone(self.timestamp.time_zone)),
            format!("DST:       {}", format_dst(self.timestamp.dst_status)),
            "".to_string(),
        ];

        let general_messages: Vec<Line<'_>> = general_messages
            .into_iter()
            .chain(format_capabilities(self.capabilities).into_iter())
            .map(|line| Line::raw(line))
            .collect();
        Paragraph::new(general_messages).block(title).render(general_area, buf);

        self.get_timer(AcpiTimerId::AcPower)
            .render("AC Power Timer", ac_area, buf);
        self.get_timer(AcpiTimerId::DcPower)
            .render("DC Power Timer", dc_area, buf);
    }
}

fn format_dst(dst: AcpiDaylightSavingsTimeStatus) -> &'static str {
    match dst {
        AcpiDaylightSavingsTimeStatus::NotObserved => "Not Observed",
        AcpiDaylightSavingsTimeStatus::NotAdjusted => "No",
        AcpiDaylightSavingsTimeStatus::Adjusted => "Yes",
    }
}

fn format_capabilities(capabilities: TimeAlarmDeviceCapabilities) -> Vec<String> {
    fn as_supported(supported: bool) -> &'static str {
        if supported { "Supported" } else { "Not Supported" }
    }
    vec![
        "Capabilities:".to_string(),
        format!(
            "  Real time:       {}",
            as_supported(capabilities.realtime_implemented())
        ),
        format!(
            "  Get Wake Status: {}",
            as_supported(capabilities.get_wake_status_supported())
        ),
        format!(
            "  Accuracy:        {}",
            if capabilities.realtime_accuracy_in_milliseconds() {
                "Milliseconds"
            } else {
                "Seconds"
            }
        ),
        format!(
            "  AC Wake:         {}",
            as_supported(capabilities.ac_wake_implemented())
        ),
        format!(
            "  AC S4 Wake:      {}",
            as_supported(capabilities.ac_s4_wake_supported())
        ),
        format!(
            "  AC S5 Wake:      {}",
            as_supported(capabilities.ac_s5_wake_supported())
        ),
        format!(
            "  DC Wake:         {}",
            as_supported(capabilities.dc_wake_implemented())
        ),
        format!(
            "  DC S4 Wake:      {}",
            as_supported(capabilities.dc_s4_wake_supported())
        ),
        format!(
            "  DC S5 Wake:      {}",
            as_supported(capabilities.dc_s5_wake_supported())
        ),
    ]
}

fn format_time(time: Datetime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        time.year(),
        u8::from(time.month()),
        time.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

fn format_time_zone(tz: AcpiTimeZone) -> String {
    match tz {
        AcpiTimeZone::Unknown => "Unknown".to_string(),
        AcpiTimeZone::MinutesFromUtc(offset) => format!(
            "UTC{:+03}:{:02}",
            offset.minutes_from_utc() / 60,
            offset.minutes_from_utc().abs() % 60
        ),
    }
}

impl<S: Source> Rtc<S> {
    pub fn new(source: S) -> Self {
        let capabilities = source.get_capabilities().unwrap();

        let mut result = Self {
            source,
            capabilities,
            timestamp: AcpiTimestamp {
                datetime: Default::default(),
                time_zone: AcpiTimeZone::Unknown,
                dst_status: AcpiDaylightSavingsTimeStatus::NotObserved,
            },
            timers: [RtcTimer::new(AcpiTimerId::AcPower), RtcTimer::new(AcpiTimerId::DcPower)],
        };

        result.update();
        result
    }

    fn get_timer(&self, timer_id: AcpiTimerId) -> &RtcTimer {
        &self.timers[timer_id as usize]
    }
}
