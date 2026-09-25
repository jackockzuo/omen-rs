use crate::error::OmenError;
use crate::protocol::command::{CommandType, CMD_LEGACY_READ};
use crate::protocol::decode::{decode_smart_adapter, SmartAdapter};
use crate::transport;

/// 读取智能适配器(0x0F)。
pub fn get() -> Result<SmartAdapter, OmenError> {
    let raw = transport::read(CMD_LEGACY_READ, CommandType::Adapter, 4)?; // ★坑1：CMD_LEGACY_READ
    decode_smart_adapter(&raw)
}

pub fn format(json: bool) -> Result<String, OmenError> {
    let a = get()?;
    if json {
        Ok(serde_json::to_string_pretty(&a)?)
    } else {
        let watts_text = match a.connected_watts {
            Some(w) => format!("{w} W"),
            None => "未知".to_string(),
        };
        Ok(format!(
            "适配器状态    : {}\n\
             圆口(barrel)  : {}\n\
             USB-C 设计功率: {} W\n\
             已连接功率    : {}",
            a.status,
            if a.barrel_port { "支持" } else { "否" },
            a.usbc_design_watts,
            watts_text,
        ))
    }
}

pub fn print(json: bool) -> Result<(), OmenError> {
    println!("{}", format(json)?);
    Ok(())
}

pub struct AdapterCommand;
impl super::Command for AdapterCommand {
    fn name(&self) -> &'static str {
        "adapter"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}
