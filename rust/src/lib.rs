use color_eyre::Result;

use time_alarm_service_messages::{
    AcpiTimerId, AcpiTimestamp, AlarmExpiredWakePolicy, AlarmTimerSeconds, TimeAlarmDeviceCapabilities, TimerStatus,
};

#[cfg(not(feature = "mock"))]
pub mod acpi;

#[cfg(feature = "mock")]
pub mod mock;

pub mod app;
pub mod battery;
pub mod common;
pub mod rtc;
pub mod thermal;
pub mod ucsi;
pub mod widgets;

/// Trait implemented by all data sources
pub trait Source: Clone {
    /// Get current temperature
    fn get_temperature(&self) -> Result<f64>;

    /// Get current fan RPM
    fn get_rpm(&self) -> Result<f64>;

    /// Get min fan RPM
    fn get_min_rpm(&self) -> Result<f64>;

    /// Get max fan RPM
    fn get_max_rpm(&self) -> Result<f64>;

    /// Get fan threshold
    fn get_threshold(&self, threshold: Threshold) -> Result<f64>;

    /// Set fan RPM limit
    fn set_rpm(&self, rpm: f64) -> Result<()>;

    /// Get battery BST data
    fn get_bst(&self) -> Result<battery::BstData>;

    /// Get battery BIX data
    fn get_bix(&self) -> Result<battery::BixData>;

    /// Set battery trippoint
    fn set_btp(&self, trippoint: u32) -> Result<()>;

    // RTC methods

    /// Get RTC capabilities bitfield - see _GCP
    fn get_capabilities(&self) -> Result<TimeAlarmDeviceCapabilities>;

    /// Get RTC time as unix timestamp - see _GRT
    fn get_real_time(&self) -> Result<AcpiTimestamp>;

    /// Query the wake status of the timer - see _GWS
    fn get_wake_status(&self, timer_id: AcpiTimerId) -> Result<TimerStatus>;

    /// Get the expired timer wake policy - see _TIP
    fn get_expired_timer_wake_policy(&self, timer_id: AcpiTimerId) -> Result<AlarmExpiredWakePolicy>;

    /// Get the timer value - see _TIV
    fn get_timer_value(&self, timer_id: AcpiTimerId) -> Result<AlarmTimerSeconds>;
}

pub enum Threshold {
    /// On threshold temperature
    On,
    /// Ramping threshold temperature
    Ramping,
    /// Max threshold temperature
    Max,
}
