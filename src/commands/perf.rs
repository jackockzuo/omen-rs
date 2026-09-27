//! 兼容层：`perf` / `unlock` / `balanced` 命令。
//!
//! 底层实现已迁到 [`crate::commands::power_profile`]（功耗配置抽象）。
//! 这里保留旧的函数签名与 `perf --json` 输出形状，供 NixOS module、旧脚本与
//! `thermal status` 继续使用。

use serde::Serialize;

use crate::commands::power_profile::{self, PowerProfile};
use crate::error::OmenError;

/// 旧版 `perf --json` 的状态形状（字段与历史版本一致）。
#[derive(Debug, Serialize)]
pub struct PerfStatus {
    pub ec_0xba: u8,
    pub ec_0x95: u8,
    pub platform_profile: Option<String>,
    pub unlocked: bool,
}

fn apply_legacy(profile: PowerProfile) -> Result<(), OmenError> {
    let report = power_profile::apply(profile, false);
    if report.ec_ok() {
        Ok(())
    } else {
        Err(OmenError::ApplyFailed(
            report.first_error().unwrap_or("未知错误").to_string(),
        ))
    }
}

pub fn unlock() -> Result<(), OmenError> {
    apply_legacy(PowerProfile::Performance)
}

pub fn balanced() -> Result<(), OmenError> {
    apply_legacy(PowerProfile::Balanced)
}

pub fn status() -> Result<PerfStatus, OmenError> {
    let s = power_profile::PowerProfileStatus::read()?;
    Ok(PerfStatus {
        ec_0xba: s.ec_0xba,
        ec_0x95: s.ec_0x95,
        platform_profile: s.platform_profile,
        unlocked: s.ec_0xba == 5,
    })
}

pub fn format_status(json: bool) -> Result<String, OmenError> {
    let s = status()?;
    if json {
        Ok(serde_json::to_string_pretty(&s)?)
    } else {
        Ok(format!(
            "EC 0xBA (功耗倍率)  : {} ({})\n\
             EC 0x95 (性能模式)  : 0x{:02X}\n\
             platform_profile    : {}",
            s.ec_0xba,
            if s.unlocked { "已解锁" } else { "未解锁" },
            s.ec_0x95,
            s.platform_profile.unwrap_or_else(|| "未知".into()),
        ))
    }
}

pub struct PerfCommand;
impl super::Command for PerfCommand {
    fn name(&self) -> &'static str {
        "perf"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format_status(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}
