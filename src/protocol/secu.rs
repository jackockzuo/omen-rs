//! SECU 缓冲区与 ACPI 参数串。
//!
//! 纯逻辑：无 I/O、无 unsafe，可单元测试。
//!
//! 布局:
//! ```text
//! 偏移   内容
//! 0x00   "SECU"              签名固定 4 字节
//! 0x04   Command   u32 小端   命令空间
//! 0x08   CommandType u32 小端 低字节是类型，其余 0
//! 0x0C   Size      u32 小端   数据长度（>=1）
//! 0x10   data                 数据
//! ```
//! 见 docs/COMMAND-REFERENCE.md §2。

use crate::protocol::command::CommandType;

/// SECU 签名。
pub const SECU_SIGN: [u8; 4] = *b"SECU";

/// 单条命令数据区上限（保守值）。
pub const MAX_DATA: usize = 112;

/// 最底层：command_type 直接是裸字节 u8。raw 探测命令用这个。
pub fn build_secu_raw(command: u32, command_type: u8, data: &[u8]) -> Vec<u8> {
    assert!(data.len() <= MAX_DATA, "SECU payload too large");
    let mut buf = Vec::with_capacity(16 + data.len());
    buf.extend_from_slice(&SECU_SIGN);
    buf.extend_from_slice(&command.to_le_bytes());
    let mut ctype = [0u8; 4];
    ctype[0] = command_type; // ← 直接用 u8，不再 .into()
    buf.extend_from_slice(&ctype);
    buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
    buf.extend_from_slice(data);
    buf
}

/// 类型安全外壳：收 CommandType，转字节后交给 raw 版。日常命令走这个。
pub fn build_secu(command: u32, command_type: CommandType, data: &[u8]) -> Vec<u8> {
    build_secu_raw(command, command_type.into(), data)
}

/// 把字节转成 ACPI 需要的字符串: `"{0x53, 0x45, ...}"`
pub fn to_acpi_hex(buf: &[u8]) -> String {
    let inner = buf
        .iter()
        .map(|b| format!("0x{b:02X}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{inner}}}")
}

/// 从 acpi 返回的文本里提取所有 `0xXX` 字节。
///
/// 对任意输入都不 panic（配合 proptest 保证）。
pub fn parse_hex_bytes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 < b.len() {
        if b[i] == b'0' && (b[i + 1] | 0x20) == b'x' {
            let hi = (b[i + 2] as char).to_digit(16);
            let lo = (b[i + 3] as char).to_digit(16);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h * 16 + l) as u8);
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn secu_layout_is_correct() {
        let b = build_secu(0x0002_0008, CommandType::SysDesign, &[0, 0, 0, 0]);
        assert_eq!(&b[0..4], b"SECU");
        assert_eq!(&b[4..8], &[0x08, 0x00, 0x02, 0x00]); // 小端！
        assert_eq!(&b[8..12], &[0x28, 0x00, 0x00, 0x00]);
        assert_eq!(&b[12..16], &[0x04, 0x00, 0x00, 0x00]);
        assert_eq!(b.len(), 20);
    }

    proptest! {
        /// 任意输入都不能让解析器 panic。
        #[test]
        fn parse_never_panics(s in ".*") {
            let _ = parse_hex_bytes(&s);
        }
    }
}
