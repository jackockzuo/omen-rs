//! 只读信息：系统能力 (0x28) + 风扇数 (0x10) + 机型 (DMI)。

use serde::Serialize;

use crate::error::OmenError;
use crate::protocol::command::{CommandType, CMD_PERF};
use crate::protocol::decode::{decode_system_design, SystemDesign};
use crate::transport;

/// 汇总的 info 数据。
#[derive(Debug, Serialize)]
pub struct Info {
    pub product: String,
    pub board: String,
    pub design: SystemDesign,
    pub fan_count: Option<u8>, // None = 读取失败/未知（区分于“0 个风扇”）
}

/// 读取全部 info（只读，安全）。
pub fn get() -> Result<Info, OmenError> {
    // 0x28：系统能力，期望 128 字节
    let design = decode_system_design(&transport::read(CMD_PERF, CommandType::SysDesign, 128)?)?;

    // 0x10：风扇数量，期望 4 字节。
    // 读取失败不致命——info 是“尽力而为”的汇总，用 None 表示未知，
    // 不要用 unwrap_or(0) 把失败伪装成“0 个风扇”。
    let fan_count = transport::read(CMD_PERF, CommandType::FanCount, 4)
        .ok()
        .and_then(|d| d.first().copied());

    Ok(Info {
        product: read_dmi("product_name").unwrap_or_else(|| "未知".into()),
        board: read_dmi("board_name").unwrap_or_else(|| "未知".into()),
        design,
        fan_count,
    })
}

pub fn format(json: bool) -> Result<String, OmenError> {
    let i = get()?;
    let d = &i.design;
    if json {
        Ok(serde_json::to_string_pretty(&i)?)
    } else {
        Ok(format!(
            "机型         : {} ({})\n\
             适配器功率   : {} W\n\
             BIOS 性能模式: {}\n\
             TGP / PPAB   : {}\n\
             热策略版本   : V{}\n\
             风扇数量     : {}\n\
             软件风扇控制 : {}\n\
             极限模式解锁 : {}\n\
             双字节 PL4   : {}\n\
             PL4 默认值   : {} W\n\
             AC LoadLine  : {} 档（默认 {}）\n\
             温度传感器   : IR={} PCH={} VR={}",
            i.product,
            i.board,
            d.adapter_watts,
            yes_no(d.supports_bios_perf()),
            yes_no(d.supports_tgp_ppab()),
            d.thermal_version,
            fan_text(i.fan_count),
            yes_no(d.sw_fan_control),
            yes_no(d.extreme_unlock),
            yes_no(d.two_byte_pl4),
            d.pl4_default,
            d.loadline_levels,
            d.loadline_default,
            yes_no(d.ir_sensor),
            yes_no(d.pch_sensor),
            yes_no(d.vr_sensor),
        ))
    }
}

pub fn print(json: bool) -> Result<(), OmenError> {
    println!("{}", format(json)?);
    Ok(())
}

/// 读 `/sys/class/dmi/id/<field>` 并去掉换行。
fn read_dmi(field: &str) -> Option<String> {
    let path = format!("/sys/class/dmi/id/{field}");
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

fn yes_no(b: bool) -> &'static str {
    if b {
        "是"
    } else {
        "否"
    }
}

fn fan_text(n: Option<u8>) -> String {
    match n {
        Some(v) => v.to_string(),
        None => "未知".into(),
    }
}

pub struct InfoCommand;
impl super::Command for InfoCommand {
    fn name(&self) -> &'static str {
        "info"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}
