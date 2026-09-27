//! 热策略 / 性能模式。0x1A {0xFF, mode}
//!   模式值见 docs/COMMAND-REFERENCE.md §9（V0/V1 不同）

use std::{fmt::Display, str::FromStr};

use serde::Serialize;

use crate::{
    capability::Cap,
    error::OmenError,
    protocol::command::{CommandType, CMD_PERF},
    transport,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalMode {
    Default,     // V0=0x00, V1 没有此项
    Balanced,    // V1=0x30, V0 映射到 Default
    Performance, // V0=0x01, V1=0x31
    Cool,        // V0=0x02, V1=0x50
    Quiet,       // V0=0x03, V1 没有此项
    Extreme,     // V0=0x04, V1=0x31
}
impl ThermalMode {
    /// 根据固件版本返回对应的字节值
    fn mode_byte(&self, thermal_version: u8) -> Result<u8, OmenError> {
        Ok(match (self, thermal_version) {
            (Self::Default, 0) => 0x00,
            (Self::Balanced, 1) => 0x30,
            (Self::Performance, 0) => 0x01,
            (Self::Performance, 1) => 0x31,
            (Self::Cool, 0) => 0x02,
            (Self::Cool, 1) => 0x50,
            (Self::Quiet, 0) => 0x03,
            (Self::Extreme, 0) => 0x04,
            (Self::Extreme, 1) => 0x31,
            _ => return Err(OmenError::Unsupported),
        })
    }
}

impl FromStr for ThermalMode {
    type Err = OmenError; // 或者用 OmenError

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // "performance" → Performance 变体
        // "balanced" → Balanced / Default（看版本）
        // "cool" → Cool
        // "quiet" → Quiet（仅 V0）
        // "extreme" → Extreme
        // 其他 → Err
        match s {
            "performance" => Ok(ThermalMode::Performance),
            "balanced" => Ok(ThermalMode::Balanced),
            "cool" => Ok(ThermalMode::Cool),
            "quiet" => Ok(ThermalMode::Quiet),
            "extreme" => Ok(ThermalMode::Extreme),
            "default" => Ok(ThermalMode::Default),
            _ => Err(OmenError::Unsupported),
        }
    }
}

impl Display for ThermalMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 把模式打印回字符串："performance"、"cool" 等
        match self {
            Self::Default => write!(f, "default"),
            Self::Balanced => write!(f, "balanced"),
            Self::Performance => write!(f, "performance"),
            Self::Cool => write!(f, "cool"),
            Self::Quiet => write!(f, "quiet"),
            Self::Extreme => write!(f, "extreme"),
        }
    }
}

pub fn set(mode: ThermalMode, thermal_version: u8) -> Result<(), OmenError> {
    let caps = crate::capability::caps()?;
    caps.ensure(Cap::BIOS_PERF)?;
    let payload = [0xFF, mode.mode_byte(thermal_version)?];
    transport::wmaa(CMD_PERF, CommandType::Thermal, &payload, 0)?;
    Ok(())
}

/// 可观测的热/性能状态。
///
/// 注意：WMAA 0x1A 是「接受无回读」的写命令，固件不提供当前热策略（cool/quiet/extreme）
/// 的读回（见 docs/COMMAND-REFERENCE.md §9）。这里退而求其次读 EC 0x95 / 0xBA 与
/// platform_profile，推导 performance / balanced / unknown。
#[derive(Debug, Serialize)]
pub struct ThermalStatus {
    pub platform_profile: Option<String>,
    pub ec_0x95: u8,
    pub ec_0xba: u8,
    pub unlocked: bool,
    pub mode: String,
}

/// 读取可观测的热/性能状态。
pub fn status() -> Result<ThermalStatus, OmenError> {
    let p = crate::commands::power_profile::PowerProfileStatus::read()?;
    let mode = match p.profile.as_str() {
        "performance" | "balanced" => p.profile.clone(),
        _ => "unknown".to_string(),
    };
    Ok(ThermalStatus {
        platform_profile: p.platform_profile,
        ec_0x95: p.ec_0x95,
        ec_0xba: p.ec_0xba,
        unlocked: p.ec_0xba == 5,
        mode,
    })
}

pub fn format_status(json: bool) -> Result<String, OmenError> {
    let s = status()?;
    if json {
        Ok(serde_json::to_string_pretty(&s)?)
    } else {
        Ok(format!(
            "热/性能模式  : {}\nplatform_profile: {}\nEC 0x95      : 0x{:02X}\nEC 0xBA      : {} ({})",
            s.mode,
            s.platform_profile.unwrap_or_else(|| "未知".into()),
            s.ec_0x95,
            s.ec_0xba,
            if s.unlocked { "已解锁" } else { "未解锁" },
        ))
    }
}

pub fn print_status(json: bool) -> Result<(), OmenError> {
    println!("{}", format_status(json)?);
    Ok(())
}

/// socket 侧只读命令：`thermal`（写命令 `thermal <mode>` 不入 registry）。
pub struct ThermalStatusCommand;
impl super::Command for ThermalStatusCommand {
    fn name(&self) -> &'static str {
        "thermal"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format_status(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn parse_performance() {
        let mode: ThermalMode = "performance".parse().unwrap();
        assert_eq!(mode, ThermalMode::Performance);
    }
    #[test]
    fn parse_unknown_fails() {
        let result = ThermalMode::from_str("turbo");
        assert!(result.is_err()); // 确认它返回了错误
    }
    #[test]
    fn performance_byte_v0() {
        let mode = ThermalMode::Performance;
        assert_eq!(mode.mode_byte(0).unwrap(), 0x01);
    }

    #[test]
    fn performance_byte_v1() {
        let mode = ThermalMode::Performance;
        assert_eq!(mode.mode_byte(1).unwrap(), 0x31);
    }
    #[test]
    fn display_roundtrip() {
        let mode = ThermalMode::Cool;
        assert_eq!(mode.to_string(), "cool");
    }
}
