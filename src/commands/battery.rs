//! 电池养护模式。0x24 {0/1, 0, 0, 0}
//!   0x01 = 开启（充电限制 80%）
//!   0x00 = 关闭
//!   注意：mode=1 会返回 0x05，必须用 mode=2（12 字节缓冲区）。

use serde::Serialize;

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

/// 电池养护状态（0x24 读，`[0]` = 0/1）。
#[derive(Debug, Serialize)]
pub struct BatteryStatus {
    pub care: bool,
}

/// 读取电池养护状态。
pub fn status() -> Result<BatteryStatus, OmenError> {
    let raw = transport::read(CMD_PERF, CommandType::BatteryCare, 4)?;
    Ok(BatteryStatus {
        care: raw.first().copied().unwrap_or(0) == 1,
    })
}

pub fn format_status(json: bool) -> Result<String, OmenError> {
    let s = status()?;
    if json {
        Ok(serde_json::to_string_pretty(&s)?)
    } else {
        Ok(if s.care {
            "电池养护已开启（充电限制 80%）".to_string()
        } else {
            "电池养护已关闭".to_string()
        })
    }
}

pub fn print_status(json: bool) -> Result<(), OmenError> {
    println!("{}", format_status(json)?);
    Ok(())
}

/// socket 侧只读命令：`battery`（写命令 `on`/`off` 不入 registry）。
pub struct BatteryStatusCommand;
impl super::Command for BatteryStatusCommand {
    fn name(&self) -> &'static str {
        "battery"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format_status(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}
