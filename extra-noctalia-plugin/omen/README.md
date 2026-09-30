# OMEN Control — Noctalia 插件

为 [Noctalia](https://noctalia.dev/)（Wayland shell/bar）提供的 HP OMEN BIOS/EC
监控与控制小组件，对接本仓库的 `omen` CLI 与 `omend` 守护进程。

## 功能

- 条形栏显示：风扇转速（RPM）、热/性能模式、omend 运行状态、WMAA 温度
  （Ambient/IR；tooltip 里另有 PCH/VR）。
- **左键：打开控制面板**。
- 右键：切换电池养护（充电限制 80%）。
- 中键：打开本小组件设置。
- 控制面板：查看 omend/风扇/温度/功耗配置/热模式/电池信息，调整
  **功耗配置**（均衡 / 性能解锁）、**热策略**（性能/均衡/清凉/安静/极限）、
  **风扇**（曲线 / 手动滑杆）、切换电池养护。
- **风扇模式**：风扇由 omend 统一管理。配置了 `OMEN_FAN_CURVE` 时默认曲线，
  未配置时默认 BIOS 自动；插件可切换 BIOS 自动、温度曲线和固定转速。
  手动/曲线档位会定期重发以防固件回退。
- **功耗配置**（`omen power profile`）：EC 0xBA 功耗倍率 + EC 0x95 性能模式 +
  platform_profile 三路协同（默认 55W ↔ 解锁 ~130W）；面板显示三路是否一致，
  不一致时标 ⚠。它与「热策略」(WMAA 0x1A) 是两个正交的轴。
- 状态不落盘：每次写操作后立即重新轮询，只显示硬件最新回报值。

## 依赖

- `omen` CLI（本仓库构建产物）在 PATH 上，或通过设置 `omen_bin` 指定绝对路径。
- **非 root 读取**：需 `omend` 运行（socket `/tmp/omend.sock`），否则读命令回退直连硬件需要 root。插件通过 `omen status --json` 单次获取硬件快照。
- **写操作**：`pkexec`（默认）或 `sudo`。

### ⚠ 写操作需要免密授权

Noctalia 的 `noctalia.runAsync` 没有控制终端，若系统里没有**交互式 Polkit agent**
（只跑 polkitd 不够），`pkexec` 会以如下错误失败：

```
Error creating textual authentication agent: Error opening current controlling
terminal for the process (`/dev/tty'): No such device or address
```

本仓库的 NixOS module（`services.omen`）默认安装一条 Polkit 规则
（`allowPasswordless = true`），让 `wheel` 组成员免密通过 pkexec 运行 `omen`，
无需 agent、无需弹窗。若不用该 module，可任选其一：

1. 手动加同样的 Polkit 规则（见 `flake.nix` 的 `security.polkit.extraConfig`）；
2. 在会话里启动一个 Polkit agent（如 `hyprpolkitagent` / `lxqt-policykit` / `polkit-gnome`）；
3. 把插件设置里的 `write_cmd` 改成 `sudo`，并在 sudoers 里为 `omen` 配 NOPASSWD。

## 安装

```bash
# 1. 注册为本地插件源。注意：path 源要指向“包含插件子目录”的目录，
#    即 extra-noctalia-plugin/（插件本体在它的 omen/ 子目录里）。
noctalia msg plugins source add omen-rs path /home/ran/Projects/omen-rs/extra-noctalia-plugin

# 2. 让 Noctalia 重新扫描插件源
noctalia msg plugins update

# 3. 启用
noctalia msg plugins enable jackockzuo/omen
```

把小组件加入 bar（编辑 `~/.config/noctalia/config.toml`），例如：

```toml
[bar.default]
start = ["launcher", "workspaces", "jackockzuo/omen:omen"]
```

> 也可以写 `omen`，Noctalia 会在 `start/center/end` 里解析为 `jackockzuo/omen:omen`。

## 设置

在 Settings → Plugins → OMEN Control（或中键点击小组件）里可配置：

| 设置 | 默认 | 说明 |
|---|---|---|
| `omen_bin` | `omen` | omen CLI 路径。**用 pkexec 写操作时请填绝对路径**（如 `/run/current-system/sw/bin/omen` 或 `~/.cargo/bin/omen`），因为 pkexec 会重置 PATH。 |
| `write_cmd` | `pkexec` | 写命令提权方式：pkexec / sudo。 |
| `poll_interval` | `2000` | 轮询间隔（毫秒）。 |
| `show_label` | `true` | 显示文字标签。 |
| `show_temps` | `true` | 显示温度。 |

## 目录结构

```
extra-noctalia-plugin/          # 插件源目录（path 源指向这里）
└── omen/                      # 单个插件目录（含 plugin.toml）
    ├── plugin.toml            # 清单：service + widget + panel + settings
    ├── service.luau           # headless 后台：轮询 omen --json，发布状态，处理写命令
    ├── widget.luau            # 条形栏小组件（薄展示客户端，左键开面板）
    ├── panel.luau             # 控制面板（信息 + 风扇/热模式/电池/解锁调整）
    └── translations/
        ├── en.json
        └── zh-Hans.json
```

## 与 omen-rs 的配合

本插件通过 `omen status --json` 向 omend 获取单次聚合快照。返回对象包含：

- `sensors`、`fan`、`power`、`thermal`、`battery`：各子命令原有 JSON 结构
- `fanMode`、`fanSpeed`、`fanCurveConfigured`、`fanCurve`：风扇期望模式与配置
- `cpuTemp`、`errors`、`updatedAtMs`：CPU 温度、读取错误和快照时间

写操作：`omen power profile set <balanced|performance>`、`omen thermal <mode>`、
`omen fan set <pct>` / `omen fan auto`、`omen battery on|off`。

说明：WMAA `0x1A`（BIOS 热策略）是「接受无回读」的写命令，固件不提供
cool/quiet/extreme 的读回；`thermal status` 因此从 `platform_profile` + EC 0x95
推导 `performance / balanced / unknown`（详见 `src/commands/thermal.rs`）。
