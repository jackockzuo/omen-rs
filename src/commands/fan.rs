//! 风扇控制。
//!   0x2E 设定转速档
//!   0x2D 读取转速档
//!   0x27 最大风扇开关
//!   0x2C 风扇类型 / 除尘能力
//!   0x2E(+128) 新版除尘

use crate::{
    capability::Cap,
    error::OmenError,
    protocol::command::{CommandType, CMD_PERF},
    transport,
};

use crate::protocol::decode::{decode_fan_status, FanStatus};

pub fn get() -> Result<FanStatus, OmenError> {
    let count_raw = transport::read(CMD_PERF, CommandType::FanCount, 4)?;
    let levels_raw = transport::read(CMD_PERF, CommandType::FanLevel, 128)?;
    decode_fan_status(&count_raw, &levels_raw)
}

pub fn format(json: bool) -> Result<String, OmenError> {
    let s = get()?;
    if json {
        Ok(serde_json::to_string_pretty(&s)?)
    } else {
        let mut out = String::new();
        for (i, fan) in s.fans.iter().enumerate() {
            out.push_str(&format!("风扇 {} 档位: {}\n", i + 1, fan.level));
        }
        Ok(out.trim_end().to_string())
    }
}

pub fn print(json: bool) -> Result<(), OmenError> {
    println!("{}", format(json)?);
    Ok(())
}

pub fn set(f1: u8, f2: u8) -> Result<(), OmenError> {
    if f1 > 100 {
        return Err(OmenError::OutOfRange {
            value: f1 as u32,
            min: 0,
            max: 100,
        });
    }
    if f2 > 100 {
        return Err(OmenError::OutOfRange {
            value: f2 as u32,
            min: 0,
            max: 100,
        });
    }
    let caps = crate::capability::caps()?;
    caps.ensure(Cap::SW_FAN_CONTROL)?;
    transport::wmaa(CMD_PERF, CommandType::FanSet, &[f1, f2], 0)?;
    Ok(())
}

pub struct FanCommand;
impl super::Command for FanCommand {
    fn name(&self) -> &'static str {
        "fan"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}

/// 恢复 BIOS 自动风扇控制（三步法，参考 omencore RestoreAutoControl）。
///
/// 1. 0x27 {0x00}      — 关满速开关
/// 2. 0x1A {0xFF, mode} — 切回 Default 热策略（V0=0x00, V1=0x30）
/// 3. 0x2E {0, 0}      — 清除手动风扇残留底值
pub fn restore_auto(thermal_version: u8) -> Result<(), OmenError> {
    let caps = crate::capability::caps()?;
    caps.ensure(Cap::SW_FAN_CONTROL)?;
    caps.ensure(Cap::BIOS_PERF)?;

    transport::wmaa(CMD_PERF, CommandType::FanMax, &[0x00], 0)?;

    let mode_byte = match thermal_version {
        0 => 0x00,
        1 => 0x30,
        _ => return Err(OmenError::Unsupported),
    };
    transport::wmaa(CMD_PERF, CommandType::Thermal, &[0xFF, mode_byte], 0)?;

    transport::wmaa(CMD_PERF, CommandType::FanSet, &[0, 0], 0)?;

    Ok(())
}
