//! GPU 控制。
//!   0x22 写 TGP/PPAB/DState/GPS
//!   0x21 读回校验
//!   0x52 显卡模式（读用 CMD_READ=1，写用 CMD_WRITE=2）

use crate::{
    capability::Cap,
    error::OmenError,
    protocol::command::{CommandType, CMD_PERF},
    protocol::decode::{decode_gpu_power, GpuPower},
    transport,
};

/// 读取 GPU 功率(0x21)。
pub fn get_power() -> Result<GpuPower, OmenError> {
    let raw = transport::read(CMD_PERF, CommandType::GpuPowerRead, 4)?; // 期望 4 字节
    decode_gpu_power(&raw) // 复用你 L2 写的解码器！
}

pub fn format(json: bool) -> Result<String, OmenError> {
    let g = get_power()?;
    if json {
        Ok(serde_json::to_string_pretty(&g)?)
    } else {
        Ok(format!(
            "GPU cTGP    : {}\n\
             GPU PPAB    : {}\n\
             GPU dState  : {}\n\
             GPS 温度阈值: {} °C",
            g.ctgp, g.ppab, g.dstate, g.gps
        ))
    }
}

pub fn print(json: bool) -> Result<(), OmenError> {
    println!("{}", format(json)?);
    Ok(())
}

pub fn set_power(ctgp: u8, ppab: u8, dstate: u8, gps: u8) -> Result<(), OmenError> {
    let caps = crate::capability::caps()?;
    caps.ensure(Cap::TGP_PPAB)?;
    transport::wmaa(
        CMD_PERF,
        CommandType::GpuPowerWrite,
        &[ctgp, ppab, dstate, gps],
        0,
    )?;
    Ok(())
}

pub struct GpuCommand;
impl super::Command for GpuCommand {
    fn name(&self) -> &'static str {
        "gpu"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}
