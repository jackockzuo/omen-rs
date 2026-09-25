//! 性能解锁：EC 0xBA 功耗倍率 + EC 0x95 性能模式 + ACPI platform_profile。
//!
//! 三个路径协同：
//!   1. EC 0xBA = 5        → 热功耗倍率（55W → 130W 解锁）
//!   2. EC 0x95 = mode byte → EC 性能模式寄存器
//!   3. platform_profile    → /sys/firmware/acpi/platform_profile（固件性能序列）
//!
//! 参考 omencore `perf --mode performance --power-limit 5`。

use crate::ec;
use crate::error::OmenError;
use serde::Serialize;
use std::fs;
use std::path::Path;

const EC_POWER_LIMIT: u16 = 0xBA;
const EC_PERF_MODE: u16 = 0x95;
const PLATFORM_PROFILE_PATH: &str = "/sys/firmware/acpi/platform_profile";

#[derive(Serialize)]
pub struct PerfStatus {
    pub ec_0xba: u8,
    pub ec_0x95: u8,
    pub platform_profile: Option<String>,
    pub unlocked: bool,
}

pub fn unlock() -> Result<(), OmenError> {
    fs::write(PLATFORM_PROFILE_PATH, "performance")
        .map_err(|_| OmenError::AcpiUnavailable)?;
    ec::write_verified(EC_POWER_LIMIT, 5)?;
    ec::write(EC_PERF_MODE, 0x05)?;
    Ok(())
}

pub fn balanced() -> Result<(), OmenError> {
    if Path::new(PLATFORM_PROFILE_PATH).exists() {
        fs::write(PLATFORM_PROFILE_PATH, "balanced").ok();
    }
    ec::write_verified(EC_POWER_LIMIT, 0)?;
    ec::write(EC_PERF_MODE, 0x01)?;
    Ok(())
}

pub fn status() -> Result<PerfStatus, OmenError> {
    let ec_0xba = ec::read(EC_POWER_LIMIT)?;
    let ec_0x95 = ec::read(EC_PERF_MODE)?;
    let platform_profile = fs::read_to_string(PLATFORM_PROFILE_PATH)
        .ok()
        .map(|s| s.trim().to_string());
    Ok(PerfStatus {
        ec_0xba,
        ec_0x95,
        platform_profile,
        unlocked: ec_0xba == 5,
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
