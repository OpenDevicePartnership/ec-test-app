use color_eyre::Result;

#[cfg(not(feature = "mock"))]
pub mod acpi;

#[cfg(feature = "mock")]
pub mod mock;

pub mod app;
pub mod battery;
pub mod common;
pub mod debug;
pub mod notifications;
pub mod rtc;
pub mod thermal;
pub mod ucsi;
pub mod widgets;

/// Trait implemented by all data sources
pub trait Source: Clone + Send + 'static {
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

    /// Get raw debug data for further processing
    fn get_dbg(&self) -> Result<Vec<u8>>;
}

pub enum Threshold {
    /// On threshold temperature
    On,
    /// Ramping threshold temperature
    Ramping,
    /// Max threshold temperature
    Max,
}
