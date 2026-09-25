//! 热策略 / 性能模式。0x1A {0xFF, mode}
//!   模式值见 docs/COMMAND-REFERENCE.md §9（V0/V1 不同）

use std::{fmt::Display, str::FromStr};

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
