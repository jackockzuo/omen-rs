//! 电池养护模式。0x24 {0/1, 0, 0, 0}
//!   0x01 = 开启（充电限制 80%）
//!   0x00 = 关闭
//!   注意：mode=1 会返回 0x05，必须用 mode=2（12 字节缓冲区）。

use crate::{
    error::OmenError,
    protocol::command::{CommandType, CMD_PERF},
    transport,
};

pub fn set_care(on: bool) -> Result<(), OmenError> {
    let data = [if on { 0x01 } else { 0x00 }, 0, 0, 0];
    transport::wmaa(CMD_PERF, CommandType::BatteryCare, &data, 4)?;
    Ok(())
}
