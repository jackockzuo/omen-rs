//! omen-rs — 与 HP OMEN BIOS/ACPI 交互的安全封装。
//!
//! 分层（自底向上）：
//!   protocol   —— 纯逻辑：SECU 拼装、命令定义、响应解码（无 I/O、无 unsafe）
//!   transport  —— 唯一碰 I/O 的层：/proc/acpi/call 读写
//!   capability —— 读 0x28 得到能力快照，作为所有写操作的门控
//!   commands   —— 面向功能的操作：组合 protocol + transport
//!
//! 约定：
//!   - 本 crate 不使用 unsafe（将来若需 /dev/port 再单独放开）。
//!   - 所有硬件返回字节都视为不可信输入。

#![forbid(unsafe_code)]
// 库代码里不允许随手 unwrap/expect（测试代码例外）。
// 只有运行 cargo clippy 时才生效，但可以当安全网。
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod capability;
pub mod client;
pub mod commands;
pub mod ec;
pub mod error;
pub mod fan_curve;
pub mod protocol;
pub mod transport;
