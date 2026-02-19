use crate::{RtcSource, Source, Threshold, common};
use battery_service_messages::{AcpiBatteryRequest, AcpiBatteryResponse, BixFixedStrings, BstReturn, Btp};
use color_eyre::{Result, eyre::eyre};
use embedded_services::relay::{MessageSerializationError, SerializableMessage};
use serialport::SerialPort;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use thermal_service_messages::{ThermalRequest, ThermalResponse};
use time_alarm_service_messages::{
    AcpiTimeAlarmRequest, AcpiTimeAlarmResponse, AcpiTimerId, AcpiTimestamp, AlarmExpiredWakePolicy, AlarmTimerSeconds,
    TimeAlarmDeviceCapabilities, TimerStatus,
};

// If it took longer than a second to receive a response, something is definitely wrong
const READ_TIMEOUT: Duration = Duration::from_millis(1000);

const SMBUS_HEADER_SZ: usize = 4;
const SMBUS_LEN_IDX: usize = 2;
const MCTP_FLAGS_IDX: usize = 7;
const MCTP_HEADER_SZ: usize = 5;
const ODP_HEADER_SZ: usize = 2; // This does not include the 2 byte command code
const HEADER_SZ: usize = SMBUS_HEADER_SZ + MCTP_HEADER_SZ + ODP_HEADER_SZ;
const CMD_CODE_SZ: usize = 2;
const BUFFER_SZ: usize = 256;
const MCTP_MAX_PACKET_LEN: usize = 69;

const THERMAL_VAR_LEN: u16 = 4;
const SENSOR_INSTANCE: u8 = 0;
const BATTERY_INSTANCE: u8 = 0;

#[derive(Clone, Copy, Debug)]
enum Destination {
    Battery,
    Thermal,
    TimeAlarm,
}

impl From<Destination> for u8 {
    fn from(dst: Destination) -> Self {
        match dst {
            Destination::Battery => 0x08,
            Destination::Thermal => 0x09,
            Destination::TimeAlarm => 0x0B,
        }
    }
}

fn prepend_headers(buffer: &mut [u8], dst: Destination, payload_sz: usize) {
    // SMBUS
    buffer[0] = 0x2;
    buffer[1] = 0xF;
    buffer[2] = (MCTP_HEADER_SZ + ODP_HEADER_SZ + payload_sz) as u8;
    buffer[3] = 0x1;

    // MCTP
    buffer[4] = 0x1;
    buffer[5] = dst.into();
    buffer[6] = 0x80;
    buffer[7] = 0xD3;
    buffer[8] = 0x7D; // Additional MCTP message type header byte

    // ODP
    buffer[9] = 1 << 1;
    buffer[10] = dst.into();
}

fn append_cmd(
    to: &mut [u8],
    from: impl SerializableMessage,
    cmd_code: u16,
) -> Result<usize, MessageSerializationError> {
    to[HEADER_SZ..HEADER_SZ + CMD_CODE_SZ].copy_from_slice(&cmd_code.to_be_bytes());
    let payload_sz = from.serialize(&mut to[HEADER_SZ + CMD_CODE_SZ..])?;
    Ok(payload_sz + CMD_CODE_SZ)
}

#[derive(Clone)]
pub struct Serial {
    port: Arc<Mutex<Box<dyn SerialPort>>>,
}

impl Serial {
    pub fn new(path: &str, baud_rate: u32, flow_control: bool) -> Self {
        let flow_control = if flow_control {
            serialport::FlowControl::Hardware
        } else {
            serialport::FlowControl::None
        };

        let port = serialport::new(path, baud_rate)
            .flow_control(flow_control)
            .timeout(READ_TIMEOUT)
            .open()
            .expect("Serial port must be available");
        port.clear(serialport::ClearBuffer::All)
            .expect("Port must be available");

        Self {
            port: Arc::new(Mutex::new(port)),
        }
    }
}

impl Serial {
    fn send<REQ: SerializableMessage + Copy, RESP: SerializableMessage>(
        &self,
        dst: Destination,
        request: REQ,
    ) -> Result<RESP> {
        let mut buffer = [0u8; BUFFER_SZ];

        // Serialize command into buffer
        let request_sz = append_cmd(&mut buffer, request, request.discriminant())
            .map_err(|e| eyre!("Serialization error: {e:?}"))?;

        // NOTE: The `mctp-rs` crate does not appear to support serializing requests and deserializing
        // responses (only the opposite), so we have to do manual serialization until that is changed.

        // And now that we know request size, serialize headers into beginning of buffer
        prepend_headers(&mut buffer, dst, request_sz);

        let mut port = self.port.lock().expect("Mutex must not be poisoned");

        // Write entire request packet
        // We first clear the input buffer in case there's anything left over if we had to bail out
        // early on previous call due to error
        port.clear(serialport::ClearBuffer::Input)
            .map_err(|e| eyre!("Serial error: {e:?}"))?;
        port.write_all(&buffer[..HEADER_SZ + request_sz])
            .map_err(|e| eyre!("Serial error: {e:?}"))?;
        port.flush().map_err(|e| eyre!("Serial error: {e:?}"))?;

        // Read response packets
        let mut response_buf = [0u8; BUFFER_SZ];
        let mut offset = 0;
        let mut cmd_code = 0;
        loop {
            // Wait for SMBUS header from response packet
            let mut buffer = [0u8; BUFFER_SZ];
            port.read_exact(&mut buffer[..SMBUS_HEADER_SZ])
                .map_err(|e| eyre!("Serial error: {e:?}"))?;

            // Get the length of the response and do a sanity check on it
            let len = buffer[SMBUS_LEN_IDX] as usize;
            if !(MCTP_HEADER_SZ..=MCTP_MAX_PACKET_LEN).contains(&len) {
                return Err(eyre!("Serial error: Invalid MCTP packet length {len}"));
            }

            // Then read rest of packet
            let packet_slice = buffer
                .get_mut(SMBUS_HEADER_SZ..SMBUS_HEADER_SZ + len)
                .ok_or_else(|| eyre!("Serial error: Response does not fit in buffer"))?;
            port.read_exact(packet_slice)
                .map_err(|e| eyre!("Serial error: {e:?}"))?;

            let flags = buffer[MCTP_FLAGS_IDX];

            // If this is a SOM packet, skip ODP header (we don't use it) and grab the command code/discriminant
            let payload_start_idx = if flags & 0x80 != 0 {
                cmd_code = u16::from_be_bytes(
                    buffer[HEADER_SZ..HEADER_SZ + CMD_CODE_SZ]
                        .try_into()
                        .expect("CMD_CODE_SZ must equal 2"),
                );
                HEADER_SZ + CMD_CODE_SZ
            } else {
                SMBUS_HEADER_SZ + MCTP_HEADER_SZ
            };

            // Finally copy the packet into our buffer used for storing the entire response at the appropriate offset
            let data_slice = &buffer[payload_start_idx..SMBUS_HEADER_SZ + len];
            let len = data_slice.len();
            response_buf[offset..offset + len].copy_from_slice(data_slice);
            offset += len;

            // If this is EOM packet, we are done
            if flags & 0x40 != 0 {
                break;
            }
        }

        RESP::deserialize(cmd_code, &response_buf).map_err(|e| eyre!("Deserialization error: {e:?}"))
    }

    fn thermal_get_var(&self, guid: uuid::Uuid) -> Result<f64> {
        let request = ThermalRequest::ThermalGetVarRequest {
            instance_id: SENSOR_INSTANCE,
            len: THERMAL_VAR_LEN,
            var_uuid: guid.to_bytes_le(),
        };
        let response = self.send(Destination::Thermal, request)?;

        if let ThermalResponse::ThermalGetVarResponse { val } = response {
            Ok(val as f64)
        } else {
            Err(eyre!("GET_VAR received wrong response"))
        }
    }

    fn thermal_set_var(&self, guid: uuid::Uuid, raw: u32) -> Result<()> {
        let request = ThermalRequest::ThermalSetVarRequest {
            instance_id: SENSOR_INSTANCE,
            len: THERMAL_VAR_LEN,
            var_uuid: guid.to_bytes_le(),
            set_var: raw,
        };
        let response = self.send(Destination::Thermal, request)?;

        if let ThermalResponse::ThermalSetVarResponse = response {
            Ok(())
        } else {
            Err(eyre!("SET_VAR received wrong response"))
        }
    }
}

impl Source for Serial {
    fn get_temperature(&self) -> Result<f64> {
        let request = ThermalRequest::ThermalGetTmpRequest {
            instance_id: SENSOR_INSTANCE,
        };
        let response = self.send(Destination::Thermal, request)?;

        if let ThermalResponse::ThermalGetTmpResponse { temperature } = response {
            Ok(common::dk_to_c(temperature))
        } else {
            Err(eyre!("GET_TMP received wrong response"))
        }
    }

    fn get_rpm(&self) -> Result<f64> {
        self.thermal_get_var(common::guid::FAN_CURRENT_RPM)
    }

    fn get_min_rpm(&self) -> Result<f64> {
        self.thermal_get_var(common::guid::FAN_MIN_RPM)
    }

    fn get_max_rpm(&self) -> Result<f64> {
        self.thermal_get_var(common::guid::FAN_MAX_RPM)
    }

    fn get_threshold(&self, threshold: Threshold) -> Result<f64> {
        let raw = match threshold {
            Threshold::On => self.thermal_get_var(common::guid::FAN_ON_TEMP),
            Threshold::Ramping => self.thermal_get_var(common::guid::FAN_RAMP_TEMP),
            Threshold::Max => self.thermal_get_var(common::guid::FAN_MAX_TEMP),
        }?;
        Ok(common::dk_to_c(raw as u32))
    }

    fn set_rpm(&self, rpm: f64) -> Result<()> {
        self.thermal_set_var(common::guid::FAN_CURRENT_RPM, rpm as u32)
    }

    fn get_bst(&self) -> Result<BstReturn> {
        let request = AcpiBatteryRequest::BatteryGetBstRequest {
            battery_id: BATTERY_INSTANCE,
        };
        let response = self.send(Destination::Battery, request)?;

        if let AcpiBatteryResponse::BatteryGetBstResponse { bst } = response {
            Ok(bst)
        } else {
            Err(eyre!("GET_BST received wrong response"))
        }
    }

    fn get_bix(&self) -> Result<BixFixedStrings> {
        let request = AcpiBatteryRequest::BatteryGetBixRequest {
            battery_id: BATTERY_INSTANCE,
        };
        let response = self.send(Destination::Battery, request)?;

        if let AcpiBatteryResponse::BatteryGetBixResponse { bix } = response {
            Ok(bix)
        } else {
            Err(eyre!("GET_BIX received wrong response"))
        }
    }

    fn set_btp(&self, trip_point: u32) -> Result<()> {
        let request = AcpiBatteryRequest::BatterySetBtpRequest {
            battery_id: BATTERY_INSTANCE,
            btp: Btp { trip_point },
        };
        let response = self.send(Destination::Battery, request)?;

        if matches!(response, AcpiBatteryResponse::BatterySetBtpResponse {}) {
            Ok(())
        } else {
            Err(eyre!("SET_BTP received wrong response"))
        }
    }
}

impl RtcSource for Serial {
    fn get_capabilities(&self) -> Result<TimeAlarmDeviceCapabilities> {
        let request = AcpiTimeAlarmRequest::GetCapabilities;
        let response = self.send(Destination::TimeAlarm, request)?;

        if let AcpiTimeAlarmResponse::Capabilities(capabilities) = response {
            Ok(capabilities)
        } else {
            Err(eyre!("GET_CAPABILITIES received wrong response"))
        }
    }

    fn get_real_time(&self) -> Result<AcpiTimestamp> {
        let request = AcpiTimeAlarmRequest::GetRealTime;
        let response = self.send(Destination::TimeAlarm, request)?;

        if let AcpiTimeAlarmResponse::RealTime(timestamp) = response {
            Ok(timestamp)
        } else {
            Err(eyre!("GET_REAL_TIME received wrong response"))
        }
    }

    fn get_wake_status(&self, timer_id: AcpiTimerId) -> Result<TimerStatus> {
        let request = AcpiTimeAlarmRequest::GetWakeStatus(timer_id);
        let response = self.send(Destination::TimeAlarm, request)?;

        if let AcpiTimeAlarmResponse::TimerStatus(status) = response {
            Ok(status)
        } else {
            Err(eyre!("GET_WAKE_STATUS received wrong response"))
        }
    }

    fn get_expired_timer_wake_policy(&self, timer_id: AcpiTimerId) -> Result<AlarmExpiredWakePolicy> {
        let request = AcpiTimeAlarmRequest::GetExpiredTimerPolicy(timer_id);
        let response = self.send(Destination::TimeAlarm, request)?;

        if let AcpiTimeAlarmResponse::WakePolicy(policy) = response {
            Ok(policy)
        } else {
            Err(eyre!("GET_TIP received wrong response"))
        }
    }

    fn get_timer_value(&self, timer_id: AcpiTimerId) -> Result<AlarmTimerSeconds> {
        let request = AcpiTimeAlarmRequest::GetTimerValue(timer_id);
        let response = self.send(Destination::TimeAlarm, request)?;

        if let AcpiTimeAlarmResponse::TimerSeconds(seconds) = response {
            Ok(seconds)
        } else {
            Err(eyre!("GET_TIV received wrong response"))
        }
    }
}
