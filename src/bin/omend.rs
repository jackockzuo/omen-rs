//! omend —— 后台守护进程。
//!
//! - 每 N 秒 hold EC（0xBA + 0x95 + platform_profile），N 由 OMEN_HOLD_INTERVAL 决定
//! - 若设置了 OMEN_FAN_CURVE，hold 后读 CPU 温度（hwmon coretemp/k10temp）→ 曲线插值 →
//!   自动调风扇；每轮都重发 0x2E（固件约 120s 后回退手动风扇，见 COMMAND-REFERENCE §13）
//! - Unix socket 接收命令，用 dyn Command 分发
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
    commands::Command,
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

fn apply_perf() {
    let mode = perf_mode();
    let result = match mode {
        "performance" => commands::perf::unlock(),
        _ => commands::perf::balanced(),
    };
    match result {
        Ok(()) => info!(mode, "hold: EC 性能已刷新"),
        Err(e) => error!(mode, error = %e, "hold: EC 刷新失败"),
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
                        None => warn!("CPU 温度不可用（coretemp/k10temp），本轮跳过风扇调整"),
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
                let cmd = std::str::from_utf8(&buf[..n])?.trim();
                let response = match cmd {
                    "status" => "omend running\n".to_string(),
                    name => match handlers.iter().find(|h| h.name() == name) {
                        Some(h) => {
                            let h_clone: Box<dyn Command> = h.boxed_clone();
                            match tokio::task::spawn_blocking(move || h_clone.run(false)).await {
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
