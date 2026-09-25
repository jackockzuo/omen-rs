//! ACPI 传输层：与 `/proc/acpi/call` 交互。
//!
//! 本机（8BAB）**只有 `WMAA`，没有 `WQAA`**，所有命令都走 `WMAA`。
//! 协议事实见 `docs/COMMAND-REFERENCE.md`。
//!
//! 职责：
//!   - 用 `protocol::secu` 拼参数串
//!   - 写 `/proc/acpi/call`，再读回结果文本
//!   - 解析 `"PASS"` + 返回码 + 数据
//!   - 全局串行锁（ACPI 并发会崩）

use crate::error::OmenError;
use crate::protocol::command::CommandType;
use crate::protocol::secu::{build_secu_raw, parse_hex_bytes, to_acpi_hex};
use std::io::{Read, Write};

/// ACPI 调用文件。
const ACPI_CALL_PATH: &str = "/proc/acpi/call";

/// 全局串行锁。ACPI 调用必须一条接一条，并发会崩内核。
static ACPI_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 根据期望的输出数据字节数，选 `WMAA` 的 mode 参数。
///
/// mode 档位（固件固定）：1→8字节, 2→12, 3→136, 4→1032, 5→4104。
/// 见 `docs/COMMAND-REFERENCE.md` §2.3。
pub fn mode_for(expected: usize) -> u8 {
    match expected {
        0 => 1,
        1..=4 => 2,
        5..=128 => 3,
        129..=1024 => 4,
        _ => 5,
    }
}

/// 发一条 `WMAA` 命令，成功时返回数据段（已去掉 `"PASS"` 和返回码）。
///
/// - `command`      : Command 空间（如 `0x0002_0008`）
/// - `command_type` : 命令类型（如 `0x28`）
/// - `data`         : 发给固件的数据（读命令惯例 `&[0, 0, 0, 0]`）
/// - `expected`     : 期望的返回数据字节数（用于选 mode）
pub fn wmaa_raw(
    command: u32,
    command_type: u8,
    data: &[u8],
    expected: usize,
) -> Result<Vec<u8>, OmenError> {
    if !std::path::Path::new(ACPI_CALL_PATH).exists() {
        return Err(OmenError::AcpiUnavailable);
    }

    // 锁里没有数据（`()`），中毒无害：上一位 panic 过的线程留下的中毒状态可直接恢复，
    // 不必让整个程序再 panic 一次。
    let _guard = ACPI_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let secu = build_secu_raw(command, command_type, data);
    let mode = mode_for(expected);
    let arg = format!("\\_SB.WMID.WMAA 0x00 0x{mode:02X} {}", to_acpi_hex(&secu));

    // 写：触发 ACPI 调用（作用域结束即关闭文件）
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .open(ACPI_CALL_PATH)?;
        f.write_all(arg.as_bytes())?;
    }

    // 读：必须用大缓冲区一次读完。
    // acpi_call 在缓冲区小于结果长度时返回 0；read_to_string 会先用小缓冲区探测而读到空。
    // 见 docs/COMMAND-REFERENCE.md §16。
    let mut f = std::fs::File::open(ACPI_CALL_PATH)?;
    let mut raw = vec![0u8; 1 << 16]; // 64 KiB
    let n = f.read(&mut raw)?;
    let text = String::from_utf8_lossy(&raw[..n]);

    let bytes = parse_hex_bytes(&text);
    if bytes.len() < 8 {
        return Err(OmenError::ShortResponse {
            got: bytes.len(),
            need: 8,
        });
    }

    let sig = &bytes[0..4];
    let rc = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if sig != b"PASS" {
        return Err(OmenError::NotPass);
    }
    if rc != 0 {
        return Err(OmenError::BiosCode(rc));
    }
    Ok(bytes[8..].to_vec())
}

/// 类型安全外壳：收 CommandType，转字节后交给 raw 版。
pub fn wmaa(
    command: u32,
    command_type: CommandType,
    data: &[u8],
    expected: usize,
) -> Result<Vec<u8>, OmenError> {
    wmaa_raw(command, command_type.into(), data, expected)
}
/// 只读辅助：读命令惯例发送 4 个 `0x00`。
pub fn read(
    command: u32,
    command_type: CommandType,
    expected: usize,
) -> Result<Vec<u8>, OmenError> {
    wmaa(command, command_type, &[0, 0, 0, 0], expected)
}
