//! 功率墙。0x29
//!   注意字节布局极易写错，见 docs/COMMAND-REFERENCE.md §6：
//!     TPP      { 0xFF, 0xFF, 0xFF, w }
//!     PL1&PL2  { w,    w,    0xFF, 0xFF }
//!     PL4      { 0xFF, 0xFF, w,    0xFF }

use crate::{
    capability::Cap,
    error::OmenError,
    protocol::command::{CommandType, CMD_PERF},
    transport,
};

pub enum PowerTarget {
    Tpp(u8),    // 总功耗包络
    Pl1Pl2(u8), // PL1 和 PL2（同时设）
    Pl4(u8),    // 峰值功耗
}

impl PowerTarget {
    fn payload(&self) -> Result<[u8; 4], OmenError> {
        let w = match self {
            Self::Tpp(w) | Self::Pl1Pl2(w) | Self::Pl4(w) => *w,
        };
        if !(10..=254).contains(&w) {
            return Err(OmenError::Unsupported); // 或者自定义错误
        }
        Ok(match self {
            Self::Tpp(w) => [0xFF, 0xFF, 0xFF, *w],
            Self::Pl1Pl2(w) => [*w, *w, 0xFF, 0xFF],
            Self::Pl4(w) => [0xFF, 0xFF, *w, 0xFF],
        })
    }
}

pub fn set(target: PowerTarget) -> Result<(), OmenError> {
    let caps = crate::capability::caps()?;
    // 写功率墙需要 BIOS_PERF 能力
    caps.ensure(Cap::BIOS_PERF)?;
    let payload = target.payload()?;
    transport::wmaa(CMD_PERF, CommandType::PowerLimits, &payload, 0)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_power_target_payload() {
        assert_eq!(
            PowerTarget::Tpp(10).payload().unwrap(),
            [0xFF, 0xFF, 0xFF, 10]
        );
        assert_eq!(
            PowerTarget::Pl1Pl2(10).payload().unwrap(),
            [10, 10, 0xFF, 0xFF]
        );
        assert_eq!(
            PowerTarget::Pl4(10).payload().unwrap(),
            [0xFF, 0xFF, 10, 0xFF]
        );
    }

    #[test]
    fn test_power_target_payload_unsupported() {
        assert!(PowerTarget::Tpp(0).payload().is_err());
        assert!(PowerTarget::Pl1Pl2(0).payload().is_err());
        assert!(PowerTarget::Pl4(0).payload().is_err());
        assert!(PowerTarget::Tpp(255).payload().is_err());
        assert!(PowerTarget::Pl1Pl2(255).payload().is_err());
        assert!(PowerTarget::Pl4(255).payload().is_err());
    }
}
