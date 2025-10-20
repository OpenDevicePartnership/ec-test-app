use crate::{Source, Threshold, common};
use color_eyre::{Result, eyre::eyre};
use std::ffi;

// This module maps the data returned from call into the C-Library to RUST structures
unsafe extern "C" {
    fn EvaluateAcpi(input: *const i8, input_len: usize, buffer: *mut u8, buf_len: &mut usize) -> i32;
}

mod guid {
    pub const _SENSOR_CRT_TEMP: uuid::Uuid = uuid::uuid!("218246e7-baf6-45f1-aa13-07e4845256b8");
    pub const _SENSOR_PROCHOT_TEMP: uuid::Uuid = uuid::uuid!("22dc52d2-fd0b-47ab-95b8-26552f9831a5");
    pub const FAN_ON_TEMP: uuid::Uuid = uuid::uuid!("ba17b567-c368-48d5-bc6f-a312a41583c1");
    pub const FAN_RAMP_TEMP: uuid::Uuid = uuid::uuid!("3a62688c-d95b-4d2d-bacc-90d7a5816bcd");
    pub const FAN_MAX_TEMP: uuid::Uuid = uuid::uuid!("dcb758b1-f0fd-4ec7-b2c0-ef1e2a547b76");
    pub const FAN_MIN_RPM: uuid::Uuid = uuid::uuid!("db261c77-934b-45e2-9742-256c62badb7a");
    pub const FAN_MAX_RPM: uuid::Uuid = uuid::uuid!("5cf839df-8be7-42b9-9ac5-3403ca2c8a6a");
    pub const FAN_CURRENT_RPM: uuid::Uuid = uuid::uuid!("adf95492-0776-4ffc-84f3-b6c8b5269683");
}

fn cstr_bytes_to_string(raw: &[u8]) -> Result<String> {
    Ok(ffi::CStr::from_bytes_until_nul(raw)
        .map_err(|_| color_eyre::eyre::eyre!("Invalid byte slice"))?
        .to_str()
        .map_err(|_| color_eyre::eyre::eyre!("String contains invalid characters"))?
        .to_owned())
}

// A user-friendly ACPI input method containing a name and optional arguments
struct AcpiMethodInput<'a, 'b> {
    name: &'a str,
    args: Option<&'b [AcpiMethodArgument]>,
}

/// A user-friendly ACPI method argument
#[derive(Debug, Copy, Clone)]
pub enum AcpiMethodArgument {
    /// Arbitrary u32 integer (DWORD)
    Int(u32),
    /// Arbitrary string
    Str(&'static str),
    /// GUID in mixed-endian format
    Guid(uuid::Bytes),
}

// Convert a user-friendly ACPI method argument to format expected by driver
impl TryFrom<AcpiMethodArgument> for AcpiMethodArgumentV1 {
    type Error = AcpiParseError;
    fn try_from(arg: AcpiMethodArgument) -> Result<Self, AcpiParseError> {
        Ok(match arg {
            AcpiMethodArgument::Guid(g) => Self {
                type_: 2,
                data_length: 16,
                data_32: 0,
                data: g.to_vec(),
            },
            AcpiMethodArgument::Str(s) => {
                let cstr = ffi::CString::new(s).map_err(|_| AcpiParseError::InvalidFormat)?;
                Self {
                    type_: 1,
                    data_length: cstr.count_bytes() as u16 + 1,
                    data_32: 0,
                    data: cstr.as_bytes_with_nul().to_vec(),
                }
            }
            AcpiMethodArgument::Int(i) => Self {
                type_: 0,
                data_length: 4,
                data_32: i,
                data: i.to_le_bytes().to_vec(),
            },
        })
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct AcpiEvalInputBufferComplexV1Ex {
    pub signature: u32,
    pub methodname: [u8; 256],
    pub size: u32,
    pub argumentcount: u32,
    pub arguments: Vec<AcpiMethodArgumentV1>,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct AcpiEvalOutputBufferV1 {
    pub signature: u32,
    pub length: u32,
    pub count: u32,
    pub arguments: Vec<AcpiMethodArgumentV1>,
}

#[repr(C)]
#[derive(Debug, Default)]
pub struct AcpiMethodArgumentV1 {
    pub type_: u16,
    pub data_length: u16,
    pub data_32: u32,
    pub data: Vec<u8>,
}

#[derive(Debug)]
pub enum AcpiParseError {
    InsufficientLength,
    InvalidFormat,
}

pub const ACPI_EVAL_INPUT_BUFFER_COMPLEX_SIGNATURE_EX: u32 = u32::from_le_bytes(*b"AeiF");

impl std::error::Error for AcpiParseError {}
impl std::fmt::Display for AcpiParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

// Convert a user-friendly ACPI input method to format expected by driver
impl TryFrom<AcpiMethodInput<'_, '_>> for AcpiEvalInputBufferComplexV1Ex {
    type Error = AcpiParseError;
    fn try_from(method: AcpiMethodInput) -> Result<Self, AcpiParseError> {
        let mut buffer = [0u8; 256];
        let bytes = method.name.as_bytes();
        let len = bytes.len().min(256);
        buffer[..len].copy_from_slice(&bytes[..len]);

        let arguments = if let Some(args) = method.args {
            args.iter()
                .map(|&arg| AcpiMethodArgumentV1::try_from(arg))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::default()
        };
        let size = arguments.iter().map(|arg| arg.data_length as u32).sum();

        Ok(AcpiEvalInputBufferComplexV1Ex {
            signature: ACPI_EVAL_INPUT_BUFFER_COMPLEX_SIGNATURE_EX,
            methodname: buffer,
            size,
            argumentcount: arguments.len() as u32,
            arguments,
        })
    }
}

// Convert ACPI input struct to a raw, packed byte buffer
impl From<AcpiEvalInputBufferComplexV1Ex> for Vec<u8> {
    fn from(input: AcpiEvalInputBufferComplexV1Ex) -> Self {
        let mut buf = Vec::new();
        buf.extend(&input.signature.to_le_bytes());
        buf.extend(&input.methodname);
        buf.extend(&input.size.to_le_bytes());
        buf.extend(&input.argumentcount.to_le_bytes());

        for arg in input.arguments.iter() {
            buf.extend(&arg.type_.to_le_bytes());
            buf.extend(&arg.data_length.to_le_bytes());
            buf.extend(&arg.data);
        }

        buf
    }
}

// Convert vec[u8] into AcpiEvalOutputBufferV1
impl TryFrom<Vec<u8>> for AcpiEvalOutputBufferV1 {
    type Error = AcpiParseError;
    fn try_from(value: Vec<u8>) -> Result<Self, AcpiParseError> {
        let signature = u32::from_le_bytes(value[0..4].try_into().map_err(|_| AcpiParseError::InvalidFormat)?);
        let length = u32::from_le_bytes(value[4..8].try_into().map_err(|_| AcpiParseError::InvalidFormat)?);
        let count = u32::from_le_bytes(value[8..12].try_into().map_err(|_| AcpiParseError::InvalidFormat)?);

        let mut offset = 12;
        let mut arguments = Vec::new();

        for _ in 0..count {
            if offset + 8 > value.len() {
                return Err(AcpiParseError::InsufficientLength);
            }

            let type_ = u16::from_le_bytes(value[offset..offset + 2].try_into().unwrap());
            let data_length = u16::from_le_bytes(value[offset + 2..offset + 4].try_into().unwrap()) as usize;
            let data_32 = if type_ == 0 {
                u32::from_le_bytes(value[offset + 4..offset + 8].try_into().unwrap())
            } else {
                0
            };
            offset += 4;

            if offset + data_length > value.len() {
                return Err(AcpiParseError::InsufficientLength);
            }

            let data = value[offset..offset + data_length].to_vec();
            offset += data_length;

            arguments.push(AcpiMethodArgumentV1 {
                type_,
                data_length: data_length as u16,
                data_32,
                data,
            });
        }

        // Now return generated content
        Ok(AcpiEvalOutputBufferV1 {
            signature,
            length,
            count,
            arguments,
        })
    }
}

#[derive(Default, Copy, Clone)]
pub struct Acpi {}

impl Acpi {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn evaluate(name: &str, args: Option<&[AcpiMethodArgument]>) -> Result<AcpiEvalOutputBufferV1, AcpiParseError> {
        // Maximum number of arguments allowed is 7 as per spec
        if let Some(args) = args
            && args.len() > 7
        {
            return Err(AcpiParseError::InsufficientLength);
        }

        let method = AcpiMethodInput { name, args };
        let input = AcpiEvalInputBufferComplexV1Ex::try_from(method)?;

        // Input buffer
        let in_buf: Vec<u8> = input.into();
        let in_buf_len = in_buf.len();

        // Output buffer
        let mut out_buf_len = 1024;
        let mut out_buf = vec![0u8; out_buf_len];

        let _res = unsafe {
            EvaluateAcpi(
                in_buf.as_ptr() as *const i8,
                in_buf_len,
                out_buf.as_mut_ptr(),
                &mut out_buf_len,
            )
        };

        AcpiEvalOutputBufferV1::try_from(out_buf)
    }
}

fn acpi_get_var(guid: uuid::Uuid) -> Result<f64> {
    let args = [AcpiMethodArgument::Int(1), AcpiMethodArgument::Guid(guid.to_bytes_le())];
    let output = Acpi::evaluate("\\_SB.ECT0.TGVR", Some(&args))?;

    if output.count != 2 {
        Err(eyre!("GET_VAR({guid}) unrecognized output"))
    } else if output.arguments[0].data_32 != 0 {
        Err(eyre!("GET_VAR({guid}) unknown failure"))
    } else {
        Ok(f64::from(output.arguments[1].data_32))
    }
}

fn acpi_set_var(guid: uuid::Uuid, value: f64) -> Result<()> {
    let value = value as u32;

    let args = [
        AcpiMethodArgument::Int(1),
        AcpiMethodArgument::Guid(guid.to_bytes_le()),
        AcpiMethodArgument::Int(value),
    ];
    let output = Acpi::evaluate("\\_SB.ECT0.TSVR", Some(&args))?;

    if output.count != 1 {
        Err(eyre!("SET_VAR({guid}, {value}) unrecognized output"))
    } else if output.arguments[0].data_32 != 0 {
        Err(eyre!("SET_VAR({guid}, {value}) unknown failure"))
    } else {
        Ok(())
    }
}

impl Source for Acpi {
    fn get_temperature(&self) -> Result<f64> {
        let output = Acpi::evaluate("\\_SB.ECT0.RTMP", None)?;
        if output.count != 1 {
            Err(eyre!("GET_TMP unrecognized output"))
        } else {
            Ok(common::dk_to_c(output.arguments[0].data_32))
        }
    }

    fn get_rpm(&self) -> Result<f64> {
        acpi_get_var(guid::FAN_CURRENT_RPM)
    }

    fn get_min_rpm(&self) -> Result<f64> {
        acpi_get_var(guid::FAN_MIN_RPM)
    }

    fn get_max_rpm(&self) -> Result<f64> {
        acpi_get_var(guid::FAN_MAX_RPM)
    }

    fn get_threshold(&self, threshold: Threshold) -> Result<f64> {
        match threshold {
            Threshold::On => Ok(common::dk_to_c(acpi_get_var(guid::FAN_ON_TEMP)? as u32)),
            Threshold::Ramping => Ok(common::dk_to_c(acpi_get_var(guid::FAN_RAMP_TEMP)? as u32)),
            Threshold::Max => Ok(common::dk_to_c(acpi_get_var(guid::FAN_MAX_TEMP)? as u32)),
        }
    }

    fn set_rpm(&self, rpm: f64) -> Result<()> {
        acpi_set_var(guid::FAN_CURRENT_RPM, rpm)
    }

    fn get_bst(&self) -> Result<crate::battery::BstData> {
        let data = Acpi::evaluate("\\_SB.ECT0.TBST", None)?;

        // We are expecting 4 32-bit values
        if data.count != 4 {
            Err(eyre!("GET_BST unrecognized output"))
        } else {
            Ok(crate::battery::BstData {
                state: crate::battery::ChargeState::try_from(data.arguments[0].data_32)?,
                rate: data.arguments[1].data_32,
                capacity: data.arguments[2].data_32,
                voltage: data.arguments[3].data_32,
            })
        }
    }

    fn get_bix(&self) -> Result<crate::battery::BixData> {
        let data = Acpi::evaluate("\\_SB.ECT0.TBIX", None)?;
        // We are expecting 21 arguments
        if data.count != 21 {
            Err(eyre!("GET_BIX unrecognized output"))
        } else {
            Ok(crate::battery::BixData {
                revision: data.arguments[0].data_32,
                power_unit: crate::battery::PowerUnit::try_from(data.arguments[1].data_32)?,
                design_capacity: data.arguments[2].data_32,
                last_full_capacity: data.arguments[3].data_32,
                battery_technology: crate::battery::BatteryTechnology::try_from(data.arguments[4].data_32)?,
                design_voltage: data.arguments[5].data_32,
                warning_capacity: data.arguments[6].data_32,
                low_capacity: data.arguments[7].data_32,
                cycle_count: data.arguments[8].data_32,
                accuracy: data.arguments[9].data_32,
                max_sample_time: data.arguments[10].data_32,
                min_sample_time: data.arguments[11].data_32,
                max_average_interval: data.arguments[12].data_32,
                min_average_interval: data.arguments[13].data_32,
                capacity_gran1: data.arguments[14].data_32,
                capacity_gran2: data.arguments[15].data_32,
                model_number: cstr_bytes_to_string(&data.arguments[16].data)?,
                serial_number: cstr_bytes_to_string(&data.arguments[17].data)?,
                battery_type: cstr_bytes_to_string(&data.arguments[18].data)?,
                oem_info: cstr_bytes_to_string(&data.arguments[19].data)?,
                swap_cap: crate::battery::SwapCap::try_from(data.arguments[20].data_32)?,
            })
        }
    }

    fn set_btp(&self, trippoint: u32) -> Result<()> {
        // No return value is expected according to ACPI spec
        let _ = Acpi::evaluate("\\_SB.ECT0.TBTP", Some(&[AcpiMethodArgument::Int(trippoint)]))?;
        Ok(())
    }

    fn get_dbg(&self) -> Result<Vec<u8>> {
        let data = Acpi::evaluate("\\_SB.ECT0.DMSG", None)?;

        // Should be expecting a single output argument: A complete defmt frame in raw bytes
        if data.count != 1 {
            Err(eyre!("DMSG unrecognized output"))
        } else {
            Ok(data.arguments[0].data.clone())
        }
    }
}
