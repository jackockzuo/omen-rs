# omen-rs

HP OMEN 笔记本 BIOS/EC 控制的 Rust 封装 —— CLI + 守护进程。

**目标机型**：OMEN by HP Gaming Laptop 16-wf0xxx（主板 8BAB）。其他机型可参考但未验证。

## 功能

| 命令 | 功能 | 底层接口 |
|---|---|---|
| `omen info` | 机型 / 硬件能力 / 热策略版本 | WMAA 0x28 + DMI |
| `omen sensors` | CPU / PCH / VR / Ambient 温度 | WMAA 0x23 |
| `omen fan` | 读风扇档位 | WMAA 0x2D |
| `omen fan set <F1> [F2]` | 手动设风扇转速（0-100%） | WMAA 0x2E |
| `omen fan auto` | 恢复 BIOS 自动控制 | 0x27+0x1A+0x2E 三步法 |
| `omen fan curve <曲线>` | 预览温度曲线插值 | — |
| `omen gpu` / `omen gpu set` | GPU 功率读写（cTGP/PPAB/DState/GPS） | WMAA 0x22 |
| `omen thermal <mode>` | 热策略（performance/balanced/cool/…） | WMAA 0x1A |
| `omen power tpp/pl1pl2/pl4 <W>` | 功率墙 | WMAA 0x29 |
| `omen adapter` | 智能适配器信息 | WMAA 0x0F |
| `omen battery on/off` | 电池养护（充电限制 80%） | WMAA 0x24 |
| `omen unlock` / `omen balanced` | EC 功耗解锁 / 恢复（55W → ~130W） | EC 0xBA/0x95 + platform_profile |
| `omen perf [--json]` | EC 性能状态 | EC RAM 读 |
| `omen raw <cmd> <type> …` | 原始 WMAA 调用（危险，需 root） | /proc/acpi/call |

守护进程 `omend`：

- **hold 看门狗**：每 N 秒重刷 EC 0xBA/0x95 + platform_profile（对抗 EC 复位）
- **温度曲线风扇控制**：读 CPU 温度（hwmon coretemp/k10temp）→ 线性插值 → 自动调风扇
  （不用 WMAA 传感器 max：PCH 空闲即 60°C+，会把风扇钉在高转速）
- **每轮重发 0x2E**：固件约 120s 后回退手动风扇（见 COMMAND-REFERENCE §13）
- **Unix socket** `/tmp/omend.sock`：读取类命令（info/sensors/fan/gpu/adapter/perf）在非 root 下自动经 socket 免 sudo 执行；`omen status` / `omen remote <cmd>` 直连 socket
- **日志**：tracing → stderr → journald（`journalctl -u omend`），级别由 `logLevel`（RUST_LOG）控制，默认 info

## 安装

**cargo**（任意 Linux 发行版）：

```bash
cargo install --git https://github.com/jackockzuo/omen-rs
```

安装 `omen` 与 `omend` 到 `~/.cargo/bin`。omend 需 root（写 EC/ACPI）；非 NixOS 用户需自行准备内核模块（`acpi_call`、`ec_sys write_support=1`）与 systemd unit。

**NixOS**（flake，推荐）：

```nix
# flake.nix
inputs.omen-rs.url = "github:jackockzuo/omen-rs";
```

```nix
# 主机配置
imports = [ inputs.omen-rs.nixosModules.default ];

services.omen = {
  enable = true;
  performance = "performance";   # 解锁 130W（"balanced" 恢复 55W）
  holdInterval = 30;             # hold 周期（秒）
  batteryCare = true;            # 电池养护
  logLevel = "info";
  fanCurve = [                   # 温度曲线（空 = BIOS 自动控制）
    { temp = 50; speed = 30; }
    { temp = 65; speed = 50; }
    { temp = 80; speed = 80; }
    { temp = 90; speed = 100; }
  ];
};
```

Module 自动配置：内核模块（`hp-wmi` / `acpi_call` / `ec_sys write_support=1`）、
开机解锁 oneshot（`omen-unlock.service`）、守护进程（`omend.service`）。

## 依赖

- Linux + root（写 `/proc/acpi/call` 与 EC debugfs）
- 内核模块：`acpi_call`（WMAA 调用）、`ec_sys write_support=1`（EC RAM 读写）、`hp-wmi`

## 开发

```bash
nix develop --command cargo test    # 46 单元 + 1 集成测试
nix develop --command cargo clippy  # 零 warning
sudo $(nix build .#default --print-out-paths)/bin/omen info
```

## 文档

- [`docs/COMMAND-REFERENCE.md`](docs/COMMAND-REFERENCE.md) —— WMAA 协议 + SSDT12 反汇编 + 本机逐条实测

## 安全设计

- `#![forbid(unsafe_code)]`
- 写操作三段式：能力门控 → 范围校验 → 写 → 读回验证
- EC 直写仅限已知寄存器（0xBA 功耗倍率 / 0x95 性能模式），写入后读回校验
