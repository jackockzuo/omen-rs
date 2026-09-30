//! omend —— 后台守护进程。
//!
//! - 每 N 秒 hold EC（0xBA + 0x95 + platform_profile），N 由 OMEN_HOLD_INTERVAL 决定
//! - 若设置了 OMEN_FAN_CURVE，每秒读 CPU 温度（hwmon coretemp/k10temp）→ 曲线插值 →
//!   自动调风扇；每轮都重发 0x2E（固件约 120s 后回退手动风扇，见 COMMAND-REFERENCE §13）
//! - Unix socket 接收命令，用 dyn Command 分发；请求行 = `<cmd> [--json]`（见 client::parse_query）。
//!   ⚠️ socket 权限 0666（任意本地用户可连），registry() 必须只放只读命令，写命令禁止入内
//! - SIGTERM 优雅退出
//! - 日志走 tracing → stderr → journald
//!
//! 环境变量（由 NixOS module 注入）：
//!   OMEN_PERF=performance|balanced  → hold 时解锁或平衡
//!   OMEN_HOLD_INTERVAL=30           → hold 周期秒数
//!   OMEN_BATTERY_CARE=1             → 开机时设电池养护
//!   OMEN_FAN_CURVE=50:30,65:50,...  → 温度曲线（不设则使用 BIOS 自动）
//!   RUST_LOG=info                   → tracing 日志级别

use omen_rs::{
    commands,
    commands::{power_profile::PowerProfile, Command},
    fan_curve::FanCurve,
    protocol::command::{CommandType, CMD_PERF},
    transport,
};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;
use tokio::signal::unix::{signal, SignalKind};
use tokio::time;
use tracing::{debug, error, info, warn};

fn init_logging() {
    // fmt::init() 的默认特性不读 RUST_LOG（本次修复的 bug）；回退 info 对应
    // NixOS module 的 logLevel 默认值，保持未设置时行为不变。
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn hold_interval() -> u64 {
    std::env::var("OMEN_HOLD_INTERVAL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30)
}

fn perf_mode() -> &'static str {
    match std::env::var("OMEN_PERF").as_deref() {
        Ok("balanced") => "balanced",
        _ => "performance",
    }
}

/// 风扇控制模式：omend 是风扇的唯一控制者，GUI/CLI 只通过这个控制文件切换。
///
/// 文件内容：
///   - `auto`        → BIOS 自动控制
///   - `curve`       → 温度曲线（仅配置 OMEN_FAN_CURVE 时可用）
///   - `manual:<0-100>` → 固定转速，每轮重发以对抗固件回退
///
/// 配置曲线时默认 `curve`，否则默认 `auto`；非法内容按 `auto`。文件模式 0666，
/// 允许用户会话里的 GUI 写入（内容会被解析校验，无法注入）。
const FAN_MODE_PATH: &str = "/run/omend-fan-mode";

enum FanMode {
    Auto,
    Curve,
    Manual(u8),
}

fn init_fan_mode(curve_configured: bool) {
    if Path::new(FAN_MODE_PATH).exists() {
        if !curve_configured
            && std::fs::read_to_string(FAN_MODE_PATH)
                .map(|s| s.trim() == "curve")
                .unwrap_or(false)
        {
            let _ = std::fs::write(FAN_MODE_PATH, "auto\n");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(FAN_MODE_PATH, std::fs::Permissions::from_mode(0o666));
        }
        return;
    }
    let initial = if curve_configured { "curve\n" } else { "auto\n" };
    if std::fs::write(FAN_MODE_PATH, initial).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(FAN_MODE_PATH, std::fs::Permissions::from_mode(0o666));
        }
    }
}

fn read_fan_mode(curve_configured: bool) -> FanMode {
    let raw = std::fs::read_to_string(FAN_MODE_PATH).unwrap_or_default();
    if raw.trim() == "auto" {
        return FanMode::Auto;
    }
    if let Some(rest) = raw.trim().strip_prefix("manual:") {
        if let Ok(n) = rest.trim().parse::<u8>() {
            return FanMode::Manual(n.min(100));
        }
    }
    if raw.trim() == "curve" && curve_configured {
        FanMode::Curve
    } else {
        FanMode::Auto
    }
}

fn status_json(curve: Option<&FanCurve>) -> String {
    use serde_json::{json, Value};

    let mut errors = Vec::new();
    let mut read = |name: &str, f: fn(bool) -> Result<String, omen_rs::error::OmenError>| {
        match f(true) {
            Ok(output) => serde_json::from_str::<Value>(&output).unwrap_or_else(|e| {
                errors.push(format!("{name}: invalid JSON: {e}"));
                Value::Null
            }),
            Err(e) => {
                errors.push(format!("{name}: {e}"));
                Value::Null
            }
        }
    };
    let sensors = read("sensors", commands::sensors::format);
    let fan = read("fan", commands::fan::format);
    let power = read("power", commands::power_profile::format_status);
    let observed_mode = power
        .get("profile")
        .and_then(Value::as_str)
        .filter(|mode| matches!(*mode, "performance" | "balanced"))
        .unwrap_or("unknown");
    let thermal = json!({
        "platform_profile": power.get("platform_profile").cloned().unwrap_or(Value::Null),
        "ec_0x95": power.get("ec_0x95").cloned().unwrap_or(Value::Null),
        "ec_0xba": power.get("ec_0xba").cloned().unwrap_or(Value::Null),
        "unlocked": power.get("ec_0xba").and_then(Value::as_u64) == Some(5),
        "mode": observed_mode,
    });
    let battery = read("battery", commands::battery::format_status);
    let cpu_temp = commands::sensors::cpu_temp();
    let sensor_values_present = sensors
        .as_object()
        .map(|values| values.values().any(|value| !value.is_null()))
        .unwrap_or(false);
    if !sensor_values_present {
        errors.push("sensors: no readable values".to_string());
    }
    if curve.is_some() && cpu_temp.is_none() {
        errors.push("fan curve: CPU temperature unavailable".to_string());
    }
    let (mode, speed) = match read_fan_mode(curve.is_some()) {
        FanMode::Auto => ("auto", None),
        FanMode::Curve => ("curve", None),
        FanMode::Manual(speed) => ("manual", Some(speed)),
    };
    let curve_points = curve.map(|c| {
        c.points()
            .iter()
            .map(|p| json!({ "temp": p.temp, "speed": p.speed }))
            .collect::<Vec<_>>()
    });
    let available = sensor_values_present || !fan.is_null() || !power.is_null() || !battery.is_null();
    let updated_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();

    json!({
        "available": available,
        "omend": true,
        "sensors": sensors,
        "cpuTemp": cpu_temp,
        "fan": fan,
        "fanMode": mode,
        "fanSpeed": speed,
        "fanCurveConfigured": curve.is_some(),
        "fanCurve": curve_points,
        "power": power,
        "thermal": thermal,
        "battery": battery,
        "errors": errors,
        "updatedAtMs": updated_at_ms,
    })
    .to_string()
}

fn apply_perf() {
    let mode = perf_mode();
    let profile = match mode {
        "performance" => PowerProfile::Performance,
        _ => PowerProfile::Balanced,
    };
    // 看门狗场景：force=true，无条件重刷三路，对抗 BIOS 自动回退。
    let report = commands::power_profile::apply(profile, true);
    if report.ec_ok() {
        info!(mode, "hold: 功耗配置已刷新");
    } else {
        error!(
            mode,
            error = report.first_error().unwrap_or("未知错误"),
            "hold: 功耗配置刷新失败"
        );
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    let interval = hold_interval();
    let mode = perf_mode();
    let fan_curve = std::env::var("OMEN_FAN_CURVE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| FanCurve::parse(&s))
        .transpose()?;
    init_fan_mode(fan_curve.is_some());

    info!(mode, interval, "omend 启动");
    if let Some(c) = &fan_curve {
        info!(curve = ?c.points(), "温度曲线风扇控制已启用");
    }

    if std::env::var("OMEN_BATTERY_CARE").as_deref() == Ok("1") {
        match commands::battery::set_care(true) {
            Ok(()) => info!("电池养护已开启"),
            Err(e) => error!(error = %e, "电池养护开启失败"),
        }
    }

    let running = Arc::new(AtomicBool::new(true));
    let last_fan_speed = Arc::new(AtomicU8::new(255));
    let last_fan_mode = Arc::new(AtomicU8::new(255));

    let r1 = Arc::clone(&running);
    let lf = Arc::clone(&last_fan_speed);
    let lm = Arc::clone(&last_fan_mode);
    let watchdog_fan_curve = fan_curve.clone();
    tokio::spawn(async move {
        let curve = watchdog_fan_curve;
        let mut last_hold: Option<tokio::time::Instant> = None;
        let mut last_fan_write: Option<tokio::time::Instant> = None;
        loop {
            if !r1.load(Ordering::SeqCst) {
                break;
            }
            let now = tokio::time::Instant::now();
            let hold_due = last_hold
                .map(|at| now.duration_since(at) >= Duration::from_secs(interval))
                .unwrap_or(true);
            let fan_watchdog_due = last_fan_write
                .map(|at| now.duration_since(at) >= Duration::from_secs(60))
                .unwrap_or(true);
            let lf = Arc::clone(&lf);
            let lm = Arc::clone(&lm);
            let curve = curve.clone();
            let hold_task = tokio::task::spawn_blocking(move || {
                if hold_due {
                    transport::read(CMD_PERF, CommandType::FanCount, 4).ok();
                    apply_perf();
                }

                match read_fan_mode(curve.is_some()) {
                    FanMode::Auto => {
                        let prev = lm.load(Ordering::SeqCst);
                        if prev != 0 {
                            match omen_rs::capability::caps().and_then(|caps| {
                                commands::fan::restore_auto(caps.design.thermal_version)
                            }) {
                                Ok(()) => {
                                    lm.store(0, Ordering::SeqCst);
                                    info!("已恢复 BIOS 自动风扇控制");
                                    true
                                }
                                Err(e) => {
                                    error!(error = %e, "恢复 BIOS 自动风扇控制失败");
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    }
                    // 模式变化时立即设置，其余时间以 60 秒间隔重发，早于固件回退。
                    FanMode::Manual(speed) => {
                        let changed = lm.load(Ordering::SeqCst) != 1 || lf.load(Ordering::SeqCst) != speed;
                        if changed || fan_watchdog_due {
                            match commands::fan::set(speed, speed) {
                                Ok(()) => {
                                    lm.store(1, Ordering::SeqCst);
                                    lf.store(speed, Ordering::SeqCst);
                                    if changed {
                                        info!(speed, "手动风扇已更新");
                                    } else {
                                        debug!(speed, "手动风扇看门狗刷新");
                                    }
                                    true
                                }
                                Err(e) => {
                                    error!(error = %e, "手动风扇设置失败");
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    }
                    // 按 CPU 结温插值；温度/目标变化时立即调整。
                    FanMode::Curve => {
                        let previous_mode = lm.swap(2, Ordering::SeqCst);
                        if let Some(c) = &curve {
                            match commands::sensors::cpu_temp() {
                                Some(temp) => {
                                    let target = c.evaluate(temp);
                                    let changed = lm.load(Ordering::SeqCst) != 2 || lf.load(Ordering::SeqCst) != target;
                                    if changed || fan_watchdog_due {
                                        match commands::fan::set(target, target) {
                                            Ok(()) => {
                                                lm.store(2, Ordering::SeqCst);
                                                lf.store(target, Ordering::SeqCst);
                                                if changed {
                                                    info!(temp, speed = target, "风扇曲线已更新");
                                                } else {
                                                    debug!(temp, speed = target, "风扇曲线看门狗刷新");
                                                }
                                                true
                                            }
                                            Err(e) => {
                                                error!(error = %e, "风扇曲线设置失败");
                                                false
                                            }
                                        }
                                    } else {
                                        false
                                    }
                                }
                                None => {
                                    if previous_mode != 2 {
                                        warn!("CPU 温度不可用（coretemp/k10temp），跳过风扇调整");
                                    }
                                    false
                                }
                            }
                        } else {
                            false
                        }
                    }
                }
            });
            match hold_task.await {
                Ok(fan_written) => {
                    let done = tokio::time::Instant::now();
                    if hold_due { last_hold = Some(done); }
                    if fan_written { last_fan_write = Some(done); }
                }
                Err(e) => error!(error = %e, "hold task 异常退出"),
            }
            time::sleep(Duration::from_secs(1)).await;
        }
    });

    let socket_path = "/tmp/omend.sock";
    if Path::new(socket_path).exists() {
        std::fs::remove_file(socket_path)?;
    }
    let listener = UnixListener::bind(socket_path)?;
    std::fs::set_permissions(socket_path, std::os::unix::fs::PermissionsExt::from_mode(0o666))?;

    let mut sigterm = signal(SignalKind::terminate())?;
    let r = Arc::clone(&running);

    let handlers = commands::registry();

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("收到 SIGTERM，退出");
                r.store(false, Ordering::SeqCst);
                break;
            }
            result = listener.accept() => {
                let (mut stream, _) = result?;
    let mut buf = [0u8; 256];
    let n = stream.read(&mut buf).await?;
    let line = std::str::from_utf8(&buf[..n])?.trim();
    let (name, json) = omen_rs::client::parse_query(line);
    let response = match name {
        "status" if json => {
            let curve = fan_curve.clone();
            match tokio::task::spawn_blocking(move || status_json(curve.as_ref())).await {
                Ok(output) => format!("{output}\n"),
                Err(e) => format!("error: status task failed: {e}\n"),
            }
        }
        "status" => "omend running\n".to_string(),
        name => match handlers.iter().find(|h| h.name() == name) {
            Some(h) => {
                let h_clone: Box<dyn Command> = h.boxed_clone();
                match tokio::task::spawn_blocking(move || h_clone.run(json)).await {
                                Ok(Ok(output)) => {
                                    info!(cmd = name, "命令执行成功");
                                    format!("{output}\n")
                                }
                                Ok(Err(e)) => {
                                    warn!(cmd = name, error = %e, "命令执行失败");
                                    format!("error: {e}\n")
                                }
                                Err(_) => {
                                    error!(cmd = name, "命令 task panic");
                                    "error: task panicked\n".to_string()
                                }
                            }
                        }
                        None => {
                            warn!(cmd = name, "未知命令");
                            "unknown command\n".to_string()
                        }
                    },
                };
                stream.write_all(response.as_bytes()).await?;
            }
        }
    }

    if let Ok(caps) = omen_rs::capability::caps() {
        omen_rs::commands::fan::restore_auto(caps.design.thermal_version).ok();
    }
    std::fs::remove_file(socket_path).ok();

    info!("omend 已退出");
    Ok(())
}
