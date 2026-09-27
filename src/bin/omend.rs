//! omend —— 后台守护进程。
//!
//! - 每 N 秒 hold EC（0xBA + 0x95 + platform_profile），N 由 OMEN_HOLD_INTERVAL 决定
//! - 若设置了 OMEN_FAN_CURVE，hold 后读 CPU 温度（hwmon coretemp/k10temp）→ 曲线插值 →
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
//!   OMEN_FAN_CURVE=50:30,65:50,...  → 温度曲线（不设则不自动调风扇）
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
///   - `curve`       → 温度曲线（默认）
///   - `manual:<0-100>` → 固定转速，每轮重发以对抗固件回退
///
/// 默认 `curve`；文件不存在/内容非法也按 `curve`。文件模式 0666，
/// 允许用户会话里的 GUI 写入（内容会被解析校验，无法注入）。
const FAN_MODE_PATH: &str = "/run/omend-fan-mode";

enum FanMode {
    Curve,
    Manual(u8),
}

fn init_fan_mode() {
    if Path::new(FAN_MODE_PATH).exists() {
        return;
    }
    if std::fs::write(FAN_MODE_PATH, "curve\n").is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(FAN_MODE_PATH, std::fs::Permissions::from_mode(0o666));
        }
    }
}

fn read_fan_mode() -> FanMode {
    let raw = std::fs::read_to_string(FAN_MODE_PATH).unwrap_or_default();
    if let Some(rest) = raw.trim().strip_prefix("manual:") {
        if let Ok(n) = rest.trim().parse::<u8>() {
            return FanMode::Manual(n.min(100));
        }
    }
    FanMode::Curve
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
    init_fan_mode();

    let interval = hold_interval();
    let mode = perf_mode();
    let fan_curve = std::env::var("OMEN_FAN_CURVE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(|s| FanCurve::parse(&s))
        .transpose()?;

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

    let r1 = Arc::clone(&running);
    let lf = Arc::clone(&last_fan_speed);
    tokio::spawn(async move {
        let curve = fan_curve.clone();
        loop {
            if !r1.load(Ordering::SeqCst) {
                break;
            }
            let lf = Arc::clone(&lf);
            let curve = curve.clone();
            tokio::task::spawn_blocking(move || {
                transport::read(CMD_PERF, CommandType::FanCount, 4).ok();
                apply_perf();

                match read_fan_mode() {
                    // GUI/CLI 指定的固定转速：每轮重发以对抗固件回退。
                    FanMode::Manual(speed) => {
                        let prev = lf.swap(speed, Ordering::SeqCst);
                        match commands::fan::set(speed, speed) {
                            Ok(()) if prev != speed => info!(speed, "手动风扇已更新"),
                            Ok(()) => debug!(speed, "手动风扇保持"),
                            Err(e) => error!(error = %e, "手动风扇设置失败"),
                        }
                    }
                    // 温度曲线（默认）：按 CPU 结温插值。
                    FanMode::Curve => {
                        if let Some(c) = &curve {
                            match commands::sensors::cpu_temp() {
                                Some(temp) => {
                                    let target = c.evaluate(temp);
                                    let prev = lf.swap(target, Ordering::SeqCst);
                                    // 无论档位是否变化都重发：固件看门狗约 120s 后回退手动风扇
                                    match commands::fan::set(target, target) {
                                        Ok(()) if prev != target => {
                                            info!(temp, speed = target, "风扇曲线已更新");
                                        }
                                        Ok(()) => debug!(temp, speed = target, "风扇曲线保持"),
                                        Err(e) => error!(error = %e, "风扇曲线设置失败"),
                                    }
                                }
                                None => {
                                    warn!("CPU 温度不可用（coretemp/k10temp），本轮跳过风扇调整")
                                }
                            }
                        }
                    }
                }
            })
            .await
            .ok();
            time::sleep(Duration::from_secs(interval)).await;
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
