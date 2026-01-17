use crate::{RtcSource, Source, Threshold, common};
use battery_service_messages::{
    BatteryState, BatterySwapCapability, BatteryTechnology, BixFixedStrings, BstReturn, PowerUnit,
};
use color_eyre::{Result, eyre::eyre};
use embedded_mcu_hal::time::{Datetime, Month, UncheckedDatetime};
use std::sync::{
    Mutex, OnceLock,
    atomic::Ordering,
    atomic::{AtomicI64, AtomicU32},
};
use time_alarm_service_messages::{
    AcpiDaylightSavingsTimeStatus, AcpiTimeZone, AcpiTimeZoneOffset, AcpiTimerId, AcpiTimestamp,
    AlarmExpiredWakePolicy, AlarmTimerSeconds, TimeAlarmDeviceCapabilities, TimerStatus,
};

static SET_RPM: AtomicI64 = AtomicI64::new(-1);
static SAMPLE: OnceLock<Mutex<(i64, i64)>> = OnceLock::new();

#[derive(Default, Copy, Clone)]
pub struct Mock {
    rtc: MockRtc,
}

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

    fn get_bst(&self) -> Result<BstReturn> {
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

        Ok(BstReturn {
            battery_state: BatteryState::from_bits(state).ok_or(eyre!("Invalid BatteryState"))?,
            battery_present_rate: 3839,
            battery_remaining_capacity: capacity,
            battery_present_voltage: 12569,
        })
    }

    fn get_bix(&self) -> Result<BixFixedStrings> {
        Ok(BixFixedStrings {
            revision: 1,
            power_unit: PowerUnit::MilliWatts,
            design_capacity: 10000,
            last_full_charge_capacity: 9890,
            battery_technology: BatteryTechnology::Primary,
            design_voltage: 13000,
            design_cap_of_warning: 5000,
            design_cap_of_low: 3000,
            cycle_count: 1337,
            measurement_accuracy: 80000,
            max_sampling_time: 42,
            min_sampling_time: 7,
            max_averaging_interval: 5,
            min_averaging_interval: 1,
            battery_capacity_granularity_1: 10,
            battery_capacity_granularity_2: 10,
            model_number: [b'4', b'2', b'.', b'0', 0, 0, 0, 0],
            serial_number: [b'1', b'2', b'3', b'-', b'4', b'5', 0, 0],
            battery_type: [b'L', b'i', b'-', b'o', b'n', 0, 0, 0],
            oem_info: [b'B', b'a', b't', b'B', b'r', b'o', b's', 0],
            battery_swapping_capability: BatterySwapCapability::ColdSwappable,
        })
    }

    fn set_btp(&self, _trippoint: u32) -> Result<()> {
        // Do nothing for mock
        Ok(())
    }
}

#[derive(Copy, Clone)]
struct MockRtc {
    time: AcpiTimestamp,
    timers: [MockRtcTimer; 2],
}

#[derive(Copy, Clone)]
struct MockRtcTimer {
    value: AlarmTimerSeconds,
    wake_policy: AlarmExpiredWakePolicy,
    timer_status: TimerStatus,
}

impl Default for MockRtcTimer {
    fn default() -> Self {
        Self {
            value: AlarmTimerSeconds(0),
            wake_policy: AlarmExpiredWakePolicy::INSTANTLY,
            timer_status: TimerStatus(0),
        }
    }
}

impl MockRtc {
    fn new() -> Self {
        Self {
            time: AcpiTimestamp {
                datetime: Datetime::new(UncheckedDatetime {
                    year: 2026,
                    month: Month::January,
                    day: 1,
                    ..Default::default()
                })
                .expect("statically known valid datetime"),
                time_zone: AcpiTimeZone::MinutesFromUtc(
                    AcpiTimeZoneOffset::new(-8 * 60).expect("statically known valid timezone"),
                ),
                dst_status: AcpiDaylightSavingsTimeStatus::NotObserved,
            },
            timers: [MockRtcTimer::default(); 2],
        }
    }

    fn get_timer(&self, timer_id: AcpiTimerId) -> &MockRtcTimer {
        &self.timers[timer_id as usize]
    }
}

impl Default for MockRtc {
    fn default() -> Self {
        Self::new()
    }
}

impl RtcSource for Mock {
    fn get_capabilities(&self) -> Result<TimeAlarmDeviceCapabilities> {
        Ok(TimeAlarmDeviceCapabilities(0xF7))
    }

    fn get_real_time(&self) -> Result<AcpiTimestamp> {
        Ok(self.rtc.time)
    }

    fn get_wake_status(&self, timer_id: AcpiTimerId) -> Result<TimerStatus> {
        Ok(self.rtc.get_timer(timer_id).timer_status)
    }

    fn get_expired_timer_wake_policy(&self, timer_id: AcpiTimerId) -> Result<AlarmExpiredWakePolicy> {
        Ok(self.rtc.get_timer(timer_id).wake_policy)
    }

    fn get_timer_value(&self, timer_id: AcpiTimerId) -> Result<AlarmTimerSeconds> {
        Ok(self.rtc.get_timer(timer_id).value)
    }
}
