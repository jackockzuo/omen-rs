//! 功耗配置：EC 0xBA 功耗倍率 + EC 0x95 性能模式 + ACPI platform_profile。
//!
//! 三条路径协同（见 docs/COMMAND-REFERENCE.md）：
//!   1. EC 0xBA = 功耗倍率（0 = 默认 55W，5 = 解锁 ~130W）
//!   2. EC 0x95 = EC 性能模式寄存器（0x01 balanced / 0x05 performance）
//!   3. platform_profile = /sys/firmware/acpi/platform_profile
//!
//! 2023+ BIOS 有看门狗会自行回退，omend 定期用 `apply(profile, force=true)` 重刷。
//! 本模块是 `perf` / `unlock` / `balanced` 的底层实现（perf.rs 只保留兼容壳）。

use crate::ec;
use crate::error::OmenError;
use serde::Serialize;
use std::fmt;
use std::fs;
use std::path::Path;
use std::str::FromStr;

const EC_POWER_LIMIT: u16 = 0xBA;
const EC_PERF_MODE: u16 = 0x95;
const PLATFORM_PROFILE_PATH: &str = "/sys/firmware/acpi/platform_profile";

/// 预设功耗配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerProfile {
    Balanced,
    Performance,
}

impl PowerProfile {
    pub fn name(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }

    fn ec_power_multiplier(self) -> u8 {
        match self {
            Self::Balanced => 0,
            Self::Performance => 5,
        }
    }

    fn ec_perf_mode(self) -> u8 {
        match self {
            Self::Balanced => 0x01,
            Self::Performance => 0x05,
        }
    }

    fn platform_profile(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Performance => "performance",
        }
    }
}

impl FromStr for PowerProfile {
    type Err = OmenError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "balanced" => Ok(Self::Balanced),
            "performance" => Ok(Self::Performance),
            _ => Err(OmenError::Unsupported),
        }
    }
}

impl fmt::Display for PowerProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// 单路径应用结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathOutcome {
    Applied,
    Skipped,
    Unavailable,
    Failed(String),
}

impl PathOutcome {
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// 一次 `apply` 的三路结果。
#[derive(Debug)]
pub struct ApplyReport {
    pub ec_0xba: PathOutcome,
    pub ec_0x95: PathOutcome,
    pub platform_profile: PathOutcome,
}

impl ApplyReport {
    /// 所有路径都成功（Skipped / Unavailable 也算成功，不阻断）。
    pub fn is_ok(&self) -> bool {
        !self.ec_0xba.is_failed() && !self.ec_0x95.is_failed() && !self.platform_profile.is_failed()
    }

    /// EC 两条路径是否成功（platform_profile 尽力而为，不纳入）。
    pub fn ec_ok(&self) -> bool {
        !self.ec_0xba.is_failed() && !self.ec_0x95.is_failed()
    }

    /// 第一条失败描述。
    pub fn first_error(&self) -> Option<&str> {
        for p in [&self.ec_0xba, &self.ec_0x95, &self.platform_profile] {
            if let PathOutcome::Failed(msg) = p {
                return Some(msg);
            }
        }
        None
    }
}

/// 可观测的功耗配置状态。
#[derive(Debug, Serialize)]
pub struct PowerProfileStatus {
    pub ec_0xba: u8,
    pub ec_0x95: u8,
    pub platform_profile: Option<String>,
    /// "performance" | "balanced" | "inconsistent" | "unknown"
    pub profile: String,
    pub consistent: bool,
    pub inconsistencies: Vec<String>,
}

fn classify(ec_0xba: u8, ec_0x95: u8, platform_profile: Option<&str>) -> (String, bool, Vec<String>) {
    let mut issues = Vec::new();

    let ba = match ec_0xba {
        0 => Some(PowerProfile::Balanced),
        5 => Some(PowerProfile::Performance),
        v => {
            issues.push(format!("EC 0xBA={v} 不是已知倍率"));
            None
        }
    };
    let p95 = match ec_0x95 {
        0x01 => Some(PowerProfile::Balanced),
        0x05 => Some(PowerProfile::Performance),
        v => {
            issues.push(format!("EC 0x95=0x{v:02X} 不是已知模式"));
            None
        }
    };
    let pp = match platform_profile {
        Some("balanced") => Some(PowerProfile::Balanced),
        Some("performance") => Some(PowerProfile::Performance),
        Some(other) => {
            issues.push(format!("platform_profile={other} 不是已知值"));
            None
        }
        None => {
            issues.push("platform_profile 不可用".to_string());
            None
        }
    };

    // 三路都读出且指向同一 profile 才算一致；缺一路/数值非法即不一致。
    let profile = match (ba, p95, pp) {
        (Some(a), Some(b), Some(c)) if a == b && b == c => a.name().to_string(),
        (None, None, None) => "unknown".to_string(),
        _ => "inconsistent".to_string(),
    };

    if profile == "inconsistent" {
        issues.push(format!(
            "三路指向不同: EC 0xBA={ec_0xba} EC 0x95=0x{ec_0x95:02X} platform_profile={}",
            platform_profile.unwrap_or("不可用")
        ));
    }

    let consistent = profile == "balanced" || profile == "performance";
    (profile, consistent, issues)
}

impl PowerProfileStatus {
    /// 读三路实际值并分类。
    pub fn read() -> Result<Self, OmenError> {
        let ec_0xba = ec::read(EC_POWER_LIMIT)?;
        let ec_0x95 = ec::read(EC_PERF_MODE)?;
        let platform_profile = fs::read_to_string(PLATFORM_PROFILE_PATH)
            .ok()
            .map(|s| s.trim().to_string());
        let (profile, consistent, inconsistencies) =
            classify(ec_0xba, ec_0x95, platform_profile.as_deref());
        Ok(Self {
            ec_0xba,
            ec_0x95,
            platform_profile,
            profile,
            consistent,
            inconsistencies,
        })
    }
}

fn apply_ec(offset: u16, target: u8, force: bool) -> Result<PathOutcome, OmenError> {
    let current = ec::read(offset)?;
    if current == target && !force {
        return Ok(PathOutcome::Skipped);
    }
    ec::write_verified(offset, target)?;
    Ok(PathOutcome::Applied)
}

fn apply_platform_profile(target: &str, force: bool) -> PathOutcome {
    if !Path::new(PLATFORM_PROFILE_PATH).exists() {
        return PathOutcome::Unavailable;
    }
    match fs::read_to_string(PLATFORM_PROFILE_PATH) {
        Ok(current) if current.trim() == target && !force => PathOutcome::Skipped,
        _ => match fs::write(PLATFORM_PROFILE_PATH, target) {
            Ok(()) => PathOutcome::Applied,
            Err(e) => PathOutcome::Failed(e.to_string()),
        },
    }
}

/// 应用功耗配置。幂等：已符合的路径跳过（`force=true` 时无条件重写，用于看门狗）。
///
/// 不会整体失败——每一条路径的结果都记录在 [`ApplyReport`] 里。
pub fn apply(profile: PowerProfile, force: bool) -> ApplyReport {
    ApplyReport {
        platform_profile: apply_platform_profile(profile.platform_profile(), force),
        ec_0x95: apply_ec(EC_PERF_MODE, profile.ec_perf_mode(), force)
            .unwrap_or_else(|e| PathOutcome::Failed(e.to_string())),
        ec_0xba: apply_ec(EC_POWER_LIMIT, profile.ec_power_multiplier(), force)
            .unwrap_or_else(|e| PathOutcome::Failed(e.to_string())),
    }
}

/// 校验三路是否一致（返回状态，调用方据 `consistent` 决定退出码）。
pub fn verify() -> Result<PowerProfileStatus, OmenError> {
    PowerProfileStatus::read()
}

pub fn format_report(profile: PowerProfile, report: &ApplyReport) -> String {
    let mut out = format!("已应用功耗配置: {profile}\n");
    let rows = [
        ("EC 0xBA", &report.ec_0xba),
        ("EC 0x95", &report.ec_0x95),
        ("platform_profile", &report.platform_profile),
    ];
    for (name, outcome) in rows {
        let desc = match outcome {
            PathOutcome::Applied => "已写".to_string(),
            PathOutcome::Skipped => "已符合，跳过".to_string(),
            PathOutcome::Unavailable => "不可用".to_string(),
            PathOutcome::Failed(e) => format!("失败: {e}"),
        };
        out.push_str(&format!("  {name:<18}: {desc}\n"));
    }
    out.trim_end().to_string()
}

pub fn format_status(json: bool) -> Result<String, OmenError> {
    let s = PowerProfileStatus::read()?;
    if json {
        Ok(serde_json::to_string_pretty(&s)?)
    } else {
        let mut out = String::new();
        out.push_str(&format!("功耗配置      : {}\n", s.profile));
        out.push_str(&format!(
            "EC 0xBA       : {} ({})\n",
            s.ec_0xba,
            if s.ec_0xba == 5 { "解锁 ~130W" } else { "默认 55W" }
        ));
        out.push_str(&format!("EC 0x95       : 0x{:02X}\n", s.ec_0x95));
        out.push_str(&format!(
            "platform_profile: {}\n",
            s.platform_profile.as_deref().unwrap_or("不可用")
        ));
        if !s.consistent {
            out.push_str("⚠ 不一致:\n");
            for i in &s.inconsistencies {
                out.push_str(&format!("  - {i}\n"));
            }
        }
        Ok(out.trim_end().to_string())
    }
}

pub fn print_status(json: bool) -> Result<(), OmenError> {
    println!("{}", format_status(json)?);
    Ok(())
}

/// socket 侧只读命令：`power`（对应 `omen power profile --json`）。
pub struct PowerProfileCommand;
impl super::Command for PowerProfileCommand {
    fn name(&self) -> &'static str {
        "power"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format_status(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_profiles() {
        assert_eq!("performance".parse::<PowerProfile>().unwrap(), PowerProfile::Performance);
        assert_eq!("balanced".parse::<PowerProfile>().unwrap(), PowerProfile::Balanced);
        assert!("turbo".parse::<PowerProfile>().is_err());
    }

    #[test]
    fn display_roundtrip() {
        assert_eq!(PowerProfile::Performance.to_string(), "performance");
        assert_eq!(PowerProfile::Balanced.to_string(), "balanced");
    }

    #[test]
    fn target_values() {
        assert_eq!(PowerProfile::Performance.ec_power_multiplier(), 5);
        assert_eq!(PowerProfile::Balanced.ec_power_multiplier(), 0);
        assert_eq!(PowerProfile::Performance.ec_perf_mode(), 0x05);
        assert_eq!(PowerProfile::Balanced.ec_perf_mode(), 0x01);
    }

    #[test]
    fn classify_performance() {
        let (profile, consistent, issues) = classify(5, 0x05, Some("performance"));
        assert_eq!(profile, "performance");
        assert!(consistent);
        assert!(issues.is_empty());
    }

    #[test]
    fn classify_balanced() {
        let (profile, consistent, _) = classify(0, 0x01, Some("balanced"));
        assert_eq!(profile, "balanced");
        assert!(consistent);
    }

    #[test]
    fn classify_inconsistent() {
        let (profile, consistent, issues) = classify(5, 0x01, Some("performance"));
        assert_eq!(profile, "inconsistent");
        assert!(!consistent);
        assert!(!issues.is_empty());
    }

    #[test]
    fn classify_unknown_when_no_platform_profile() {
        let (profile, consistent, issues) = classify(0, 0x01, None);
        assert_eq!(profile, "inconsistent");
        assert!(!consistent);
        assert!(issues.iter().any(|i| i.contains("platform_profile")));
    }
}
