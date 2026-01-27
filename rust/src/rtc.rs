use crate::common;
use color_eyre::Result;
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
const DATA_NOT_YET_RETRIEVED_MSG: &str = "Data not yet retrieved";

mod rtc_timer {
    use super::*;
    pub struct RtcTimer {
        timer_id: AcpiTimerId,

        value: Result<AlarmTimerSeconds>,
        wake_policy: Result<AlarmExpiredWakePolicy>,
        timer_status: Result<TimerStatus>,
    }

    impl RtcTimer {
        pub fn update(&mut self, source: &impl RtcSource) {
            self.value = source.get_timer_value(self.timer_id);
            self.wake_policy = source.get_expired_timer_wake_policy(self.timer_id);
            self.timer_status = source.get_wake_status(self.timer_id);
        }

        pub fn new(timer_id: AcpiTimerId) -> Self {
            Self {
                timer_id,
                value: Err(color_eyre::eyre::eyre!(DATA_NOT_YET_RETRIEVED_MSG)),
                wake_policy: Err(color_eyre::eyre::eyre!(DATA_NOT_YET_RETRIEVED_MSG)),
                timer_status: Err(color_eyre::eyre::eyre!(DATA_NOT_YET_RETRIEVED_MSG)),
            }
        }

        pub fn render(&self, title: &str, area: Rect, buf: &mut Buffer) {
            let is_healthy = self.value.is_ok() && self.wake_policy.is_ok() && self.timer_status.is_ok();
            let title = common::title_str_with_status(title, is_healthy);

            Paragraph::new(vec![
                Line::raw(format_result("Time remaining: ", &self.value, |value| match *value {
                    AlarmTimerSeconds::DISABLED => "Timer not set".to_string(),
                    seconds => format!("{} seconds", seconds.0),
                })),
                Line::raw(format_result(
                    "Wake policy:    ",
                    &self.wake_policy,
                    |wake_policy| match *wake_policy {
                        AlarmExpiredWakePolicy::NEVER => "never".to_string(),
                        AlarmExpiredWakePolicy::INSTANTLY => "instantly".to_string(),
                        wake_policy => format!("after {} seconds", wake_policy.0),
                    },
                )),
                Line::raw(format_result("Timer status:   ", &self.timer_status, |timer_status| {
                    format!(
                        "{}, {}",
                        if timer_status.timer_expired() {
                            "expired".to_string()
                        } else {
                            "not expired".to_string()
                        },
                        if timer_status.timer_triggered_wake() {
                            "triggered wake".to_string()
                        } else {
                            "did not trigger wake".to_string()
                        }
                    )
                })),
            ])
            .block(common::title_block(&title, 0, LABEL_COLOR))
            .render(area, buf);
        }
    }

    fn format_result<T>(label: &str, res: &Result<T>, f: impl FnOnce(&T) -> String) -> String {
        match res {
            Ok(value) => format!("{}{}", label, f(value)),
            Err(err) => format!("{}Error: {}", label, err),
        }
    }
}

use rtc_timer::RtcTimer;

pub struct Rtc<S: Source> {
    source: S,
    timers: [RtcTimer; 2],

    capabilities: Result<TimeAlarmDeviceCapabilities>,
    timestamp: Result<AcpiTimestamp>,
}

impl<S: Source> Module for Rtc<S> {
    fn title(&self) -> &'static str {
        "RTC Information"
    }

    fn update(&mut self) {
        // Capabilities should be static, so don't try to update after a successful fetch
        if self.capabilities.is_err() {
            self.capabilities = self.source.get_capabilities();
        }
        self.timestamp = self.source.get_real_time();
        for timer in &mut self.timers {
            timer.update(&self.source);
        }
    }

    fn handle_event(&mut self, _evt: &Event) {}

    fn render(&self, area: Rect, buf: &mut Buffer) {
        let is_healthy = self.capabilities.is_ok() && self.timestamp.is_ok();
        let title = common::title_str_with_status("Real-time Clock", is_healthy);
        let title = common::title_block(&title, 0, LABEL_COLOR);

        let [general_area, timers_area] = common::area_split(area, Direction::Vertical, 70, 30);
        let [ac_area, dc_area] = common::area_split(timers_area, Direction::Horizontal, 50, 50);

        let time_messages = match &self.timestamp {
            Ok(timestamp) => vec![
                format!("Time:      {}", format_time(timestamp.datetime)),
                format!("Time Zone: {}", format_time_zone(timestamp.time_zone)),
                format!("DST:       {}", format_dst(timestamp.dst_status)),
                "".to_string(),
            ],
            Err(err) => vec![format!("Error retrieving RTC time: {}", err)],
        };

        let capabilities_messages: Vec<String> = match &self.capabilities {
            Ok(capabilities) => format_capabilities(capabilities),
            Err(err) => vec![format!("Error retrieving RTC capabilities: {}", err)],
        };

        let all_messages: Vec<Line<'_>> = time_messages
            .into_iter()
            .chain(capabilities_messages)
            .map(Line::raw)
            .collect();

        Paragraph::new(all_messages).block(title).render(general_area, buf);

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

fn format_capabilities(capabilities: &TimeAlarmDeviceCapabilities) -> Vec<String> {
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
        let mut result = Self {
            source,
            capabilities: Err(color_eyre::eyre::eyre!(DATA_NOT_YET_RETRIEVED_MSG)),
            timestamp: Err(color_eyre::eyre::eyre!(DATA_NOT_YET_RETRIEVED_MSG)),
            timers: [RtcTimer::new(AcpiTimerId::AcPower), RtcTimer::new(AcpiTimerId::DcPower)],
        };

        result.update();
        result
    }

    fn get_timer(&self, timer_id: AcpiTimerId) -> &RtcTimer {
        &self.timers[timer_id as usize]
    }
}
