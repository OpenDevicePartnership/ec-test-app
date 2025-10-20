use crate::{Source, Threshold, common};
use color_eyre::Result;
use std::sync::{
    Mutex, OnceLock,
    atomic::{AtomicI64, AtomicU16, AtomicU32, AtomicU64, Ordering},
};

static SET_RPM: AtomicI64 = AtomicI64::new(-1);
static SAMPLE: OnceLock<Mutex<(i64, i64)>> = OnceLock::new();

// Produces a fake "on-the-wire" byte representation of a defmt call that matches format expected by mock-bin
// Index is equal to the address of the log string in the `.defmt` section of mock-bin ELF
// Mainly used for testing defmt decoding
fn mock_defmt_wire(index: u16, timestamp: u64) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.extend(index.to_le_bytes());
    buf.extend(timestamp.to_le_bytes());
    let mut buf = rzcobs::encode(&buf);

    // End frame delimeter
    buf.push(0x00);
    buf
}

#[derive(Default, Copy, Clone)]
pub struct Mock {}

impl Mock {
    pub fn new() -> Self {
        Default::default()
    }
}

impl Source for Mock {
    fn get_temperature(&self) -> Result<f64> {
        let mut sample = SAMPLE.get_or_init(|| Mutex::new((2732, 1))).lock().unwrap();

        sample.0 += 10 * sample.1;
        if sample.0 >= 3232 || sample.0 <= 2732 {
            sample.1 *= -1;
        }

        Ok(common::dk_to_c(sample.0 as u32))
    }

    fn get_rpm(&self) -> Result<f64> {
        use std::f64::consts::PI;
        use std::sync::{Mutex, OnceLock};

        // For mock, if user sets RPM, we just always return what was last set instead of sin wave
        let set_rpm = SET_RPM.load(Ordering::Relaxed);
        if set_rpm >= 0 {
            Ok(set_rpm as f64)
        } else {
            // Generate sin wave
            static SAMPLE: OnceLock<Mutex<f64>> = OnceLock::new();
            let mut sample = SAMPLE.get_or_init(|| Mutex::new(0.0)).lock().unwrap();

            let freq = 0.1;
            let amplitude = 3000.0;
            let base = 3000.0;
            let rpm = (sample.sin() * amplitude) + base;

            *sample += freq;
            if *sample > 2.0 * PI {
                *sample -= 2.0 * PI;
            }

            Ok(rpm)
        }
    }

    fn get_min_rpm(&self) -> Result<f64> {
        Ok(0.0)
    }

    fn get_max_rpm(&self) -> Result<f64> {
        Ok(6000.0)
    }

    fn get_threshold(&self, threshold: Threshold) -> Result<f64> {
        match threshold {
            Threshold::On => Ok(28.0),
            Threshold::Ramping => Ok(40.0),
            Threshold::Max => Ok(44.0),
        }
    }

    fn set_rpm(&self, rpm: f64) -> Result<()> {
        SET_RPM.store(rpm as i64, Ordering::Relaxed);
        Ok(())
    }

    fn get_bst(&self) -> Result<crate::battery::BstData> {
        static STATE: AtomicU32 = AtomicU32::new(2);
        const MAX_CAPACITY: u32 = 10000;
        static CAPACITY: AtomicU32 = AtomicU32::new(0);
        const RATE: u32 = 1000;

        let state = STATE.load(Ordering::Relaxed);
        let capacity = CAPACITY.load(Ordering::Relaxed);
        let mut new_capacity = capacity;

        // We are only using atomics to satisfy borrow-checker
        // Thus we update non-atomically for simplicity
        if state == 2 {
            new_capacity += RATE;
            if new_capacity > MAX_CAPACITY {
                STATE.store(1, Ordering::Relaxed);
            }
        } else {
            new_capacity -= RATE;
            if new_capacity < RATE {
                STATE.store(2, Ordering::Relaxed);
            }
        }
        CAPACITY.store(new_capacity.clamp(0, MAX_CAPACITY), Ordering::Relaxed);

        Ok(crate::battery::BstData {
            state: crate::battery::ChargeState::try_from(state)?,
            rate: 3839,
            capacity,
            voltage: 12569,
        })
    }

    fn get_bix(&self) -> Result<crate::battery::BixData> {
        Ok(crate::battery::BixData {
            revision: 1,
            power_unit: crate::battery::PowerUnit::Mw,
            design_capacity: 10000,
            last_full_capacity: 9890,
            battery_technology: crate::battery::BatteryTechnology::Primary,
            design_voltage: 13000,
            warning_capacity: 5000,
            low_capacity: 3000,
            cycle_count: 1337,
            accuracy: 80000,
            max_sample_time: 42,
            min_sample_time: 7,
            max_average_interval: 5,
            min_average_interval: 1,
            capacity_gran1: 10,
            capacity_gran2: 10,
            model_number: "42.0".to_string(),
            serial_number: "123-45-678".to_string(),
            battery_type: "Li-ion".to_string(),
            oem_info: "Battery Bros.".to_string(),
            swap_cap: crate::battery::SwapCap::ColdSwappable,
        })
    }

    fn set_btp(&self, _trippoint: u32) -> Result<()> {
        // Do nothing for mock
        Ok(())
    }

    fn get_dbg(&self) -> Result<Vec<u8>> {
        const DEFMT_START: u16 = 1;
        const DEFMT_END: u16 = 6;
        static DEFMT_IDX: AtomicU16 = AtomicU16::new(DEFMT_START);
        static TIMESTAMP: AtomicU64 = AtomicU64::new(0);

        let frame_idx = DEFMT_IDX.fetch_add(1, Ordering::Relaxed);
        let timestamp = TIMESTAMP.fetch_add(100000, Ordering::Relaxed);
        let frame = mock_defmt_wire(frame_idx, timestamp);

        let _ = DEFMT_IDX.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |idx| {
            (idx > DEFMT_END).then_some(DEFMT_START)
        });

        Ok(frame)
    }
}
