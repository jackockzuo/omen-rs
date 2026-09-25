use anyhow::Context;
use clap::{Parser, Subcommand};
use omen_rs::commands::{power::PowerTarget, thermal::ThermalMode};
use std::str::FromStr;
#[derive(Parser)]
#[command(name = "omen", version, about = "OMEN BIOS 安全封装")]
struct Cli {
    /// 以 JSON 格式输出（机器可读）
    #[arg(global = true, long)]
    json: bool,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum FanSub {
    /// 设置风扇转速（百分比）
    Set { f1: u8, f2: Option<u8> },
    /// 恢复 BIOS 自动风扇控制
    Auto,
    /// 预览温度曲线（格式: "50:30,65:50,80:80,90:100"）
    Curve { curve: String },
}

#[derive(Subcommand)]
enum GpuSub {
    /// 设置 GPU 功率参数（cTGP PPAB DState GPS）
    Set { ctgp: u8, ppab: u8, dstate: u8, gps: u8 },
}

#[derive(Subcommand)]
enum BatterySub {
    /// 开启电池养护（充电限制在 80%）
    On,
    /// 关闭电池养护
    Off,
}

#[derive(Subcommand)]
enum Cmd {
    /// 显示系统信息与硬件能力
    Info,
    /// 显示传感器温度
    Sensors,
    /// GPU 功率读/写
    Gpu {
        #[command(subcommand)]
        sub: Option<GpuSub>,
    },
    /// 显示风扇转速
    Fan {
        #[command(subcommand)]
        sub: Option<FanSub>,
    },
    /// 显示智能适配器信息
    Adapter,
    /// 电池养护模式
    Battery {
        #[command(subcommand)]
        sub: BatterySub,
    },
    /// 查询 omend 守护进程状态
    Status,
    /// 解锁性能（EC 0xBA=5 + platform_profile=performance）
    Unlock,
    /// 恢复平衡模式
    Balanced,
    /// EC 性能状态（0xBA/0x95/platform_profile）
    Perf,
    /// 通过 omend 远程执行命令
    Remote { cmd: String },
    /// RAW
    Raw {
        /// Command 空间，hex（如 0x20008 或 1）
        command: String,
        /// CommandType，hex（如 0x28）
        ctype: String,
        /// mode 档位 1-5（返回缓冲区大小）
        mode: u8,
        /// 数据字节，hex（如 00 00 00 00）
        data: Vec<String>,
    },
    Thermal {
        mode: String,
    },
    Power {
        target: String,
        watts: u8,
    },
}

/// 读取类命令统一路由：root 直连硬件；非 root 经 omend socket（免 sudo）。
/// socket 不可用时回退直连，报错里带上原因提示。
fn run_read<E, F>(name: &str, json: bool, direct: F) -> anyhow::Result<()>
where
    F: FnOnce(bool) -> Result<(), E>,
    E: std::error::Error + Send + Sync + 'static,
{
    if omen_rs::client::is_root() {
        return direct(json).map_err(Into::into);
    }
    match omen_rs::client::query_via_daemon(name, json) {
        Some(resp) => {
            print!("{resp}");
            Ok(())
        }
        None => direct(json).map_err(|e| {
            anyhow::Error::from(e)
                .context("未 root 且 omend 未运行：sudo 运行，或 systemctl start omend")
        }),
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Info => run_read("info", cli.json, omen_rs::commands::info::print)
            .context("读取系统信息失败")?,
        Cmd::Sensors => run_read("sensors", cli.json, omen_rs::commands::sensors::print)
            .context("读取传感器信息失败")?,
        Cmd::Gpu { sub } => match sub {
            None => run_read("gpu", cli.json, omen_rs::commands::gpu::print)
                .context("读取 GPU 信息失败")?,
            Some(GpuSub::Set { ctgp, ppab, dstate, gps }) => {
                omen_rs::commands::gpu::set_power(ctgp, ppab, dstate, gps)
                    .context("设置 GPU 功率失败")?;
                println!("GPU 功率已设: cTGP={} PPAB={} DState={} GPS={}", ctgp, ppab, dstate, gps);
            }
        },
        Cmd::Fan { sub } => match sub {
            None => run_read("fan", cli.json, omen_rs::commands::fan::print)
                .context("读取风扇转速失败")?,
            Some(FanSub::Set { f1, f2 }) => match f2 {
                Some(speed) => {
                    omen_rs::commands::fan::set(f1, speed).context("设置风扇转速失败")?;
                    println!("风扇转速已设: {}% / {}%", f1, speed);
                }
                None => {
                    omen_rs::commands::fan::set(f1, f1).context("设置风扇转速失败")?;
                    println!(
                        "风扇转速已设: {}% / {}% (Fan 2 已与 Fan 1 保持一致）",
                        f1, f1
                    );
                }
            },
            Some(FanSub::Auto) => {
                let caps = omen_rs::capability::caps().context("读取本机能力失败")?;
                let design = &caps.design;
                omen_rs::commands::fan::restore_auto(design.thermal_version)
                    .context("恢复自动风扇控制失败")?;
                println!("已尝试恢复自动风扇控制");
                println!("如果风扇仍固定转速，请重启电脑恢复");
            }
            Some(FanSub::Curve { curve }) => {
                let c = omen_rs::fan_curve::FanCurve::parse(&curve)
                    .context("解析温度曲线失败")?;
                println!("温度曲线:");
                for p in c.points() {
                    println!("  {:>3}°C → {:>3}%", p.temp, p.speed);
                }
                println!("预览:");
                for temp in (40..=95u8).step_by(5) {
                    let speed = c.evaluate(temp);
                    println!("  {temp:>3}°C → {speed:>3}%");
                }
                println!("omend 用法: OMEN_FAN_CURVE=\"{curve}\"");
            }
        },
        Cmd::Adapter => run_read("adapter", cli.json, omen_rs::commands::adapter::print)
            .context("读取智能适配器失败")?,
        Cmd::Battery { sub } => match sub {
            BatterySub::On => {
                omen_rs::commands::battery::set_care(true).context("开启电池养护失败")?;
                println!("电池养护已开启（充电限制 80%）");
            }
            BatterySub::Off => {
                omen_rs::commands::battery::set_care(false).context("关闭电池养护失败")?;
                println!("电池养护已关闭");
            }
        },
        Cmd::Status => match omen_rs::client::send("status") {
            Ok(resp) => print!("{resp}"),
            Err(_) => println!("omend 未运行"),
        },
        Cmd::Unlock => {
            omen_rs::commands::perf::unlock().context("性能解锁失败")?;
            println!("性能已解锁 (EC 0xBA=5, platform_profile=performance)");
        },
        Cmd::Balanced => {
            omen_rs::commands::perf::balanced().context("恢复平衡失败")?;
            println!("已恢复平衡模式 (EC 0xBA=0, platform_profile=balanced)");
        },
        Cmd::Perf => run_read("perf", cli.json, |json| {
            omen_rs::commands::perf::format_status(json).map(|out| println!("{out}"))
        })
        .context("读取 EC 性能状态失败")?,
        Cmd::Remote { cmd } => {
            let payload = if cli.json { format!("{cmd} --json") } else { cmd };
            match omen_rs::client::send(&payload) {
                Ok(resp) => print!("{resp}"),
                Err(_) => println!("omend 未运行"),
            }
        },
        Cmd::Raw {
            command,
            ctype,
            mode,
            data,
        } => omen_rs::commands::raw::run(&command, &ctype, mode, &data)
            .context("执行 RAW 指令失败")?,
        Cmd::Thermal { mode } => {
            let parsed: ThermalMode =
                ThermalMode::from_str(&mode).context("解析热策略模式失败")?;
            let caps = omen_rs::capability::caps().context("读取本机能力失败")?;
            let design = &caps.design;
            omen_rs::commands::thermal::set(parsed, design.thermal_version)
                .context("设置热策略失败")?;
            println!("热策略已设为: {} (V{})", mode, design.thermal_version);
        }
        Cmd::Power { target, watts } => {
            let t = match target.as_str() {
                "tpp" => PowerTarget::Tpp(watts),
                "pl1pl2" => PowerTarget::Pl1Pl2(watts),
                "pl4" => PowerTarget::Pl4(watts),
                _ => {
                    return Err(anyhow::anyhow!(
                        "未知功率目标: {}（可选: tpp/pl1pl2/pl4）",
                        target
                    ))
                }
            };
            omen_rs::commands::power::set(t).context("设置功率墙失败")?;
            println!("功率已设: {} = {}W", target, watts);
        }
    }
    Ok(())
}
