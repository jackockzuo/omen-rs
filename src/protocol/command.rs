//! 命令定义：Command 空间 + CommandType。
//! 见 `docs/COMMAND-REFERENCE.md`。

/// Command 空间
pub const CMD_PERF: u32 = 0x0002_0008; // 性能/风扇/温度/系统
pub const CMD_LIGHTING: u32 = 0x0002_0009; // 灯效
pub const CMD_LEGACY_READ: u32 = 0x0000_0001; // 适配器/显卡模式读
pub const CMD_WRITE: u32 = 0x0000_0002; // 显卡模式写

use num_enum::{IntoPrimitive, TryFromPrimitive};

/// 命令类型(固件功能号)。每个变体绑定它在协议里的字节值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u8)]
pub enum CommandType {
    Adapter = 0x0F,
    FanCount = 0x10,
    Thermal = 0x1A,
    BatteryCare = 0x24,
    FanMax = 0x27,
    Temp = 0x23,
    SysDesign = 0x28,
    PowerLimits = 0x29,
    FanLevel = 0x2D,
    FanSet = 0x2E,
    GpuPowerRead = 0x21,
    GpuPowerWrite = 0x22,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discriminants_match_protocol() {
        // 钉死字节值:万一打错成 0x24，这里立刻 FAIL，不用等到硬件
        assert_eq!(u8::from(CommandType::Temp), 0x23);
        assert_eq!(u8::from(CommandType::SysDesign), 0x28);
        assert_eq!(u8::from(CommandType::FanMax), 0x27);
        assert_eq!(u8::from(CommandType::BatteryCare), 0x24);
        assert_eq!(u8::from(CommandType::FanSet), 0x2E);
        assert_eq!(u8::from(CommandType::GpuPowerRead), 0x21);
        assert_eq!(u8::from(CommandType::GpuPowerWrite), 0x22);
    }

    #[test]
    fn try_from_accepts_known_byte() {
        assert_eq!(CommandType::try_from(0x23).unwrap(), CommandType::Temp);
    }

    #[test]
    fn try_from_rejects_unknown_byte() {
        assert!(CommandType::try_from(0x99).is_err()); // 非法命令被挡住
    }
}
