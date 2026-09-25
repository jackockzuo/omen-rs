//! 统一错误类型（thiserror）。
//!
//! 上层（bin）可用 anyhow::Context 补充语境。

/// omen-rs 的统一错误类型。
#[derive(thiserror::Error, Debug)]
pub enum OmenError {
    /// /proc/acpi/call 不存在（acpi_call 内核模块没加载）
    #[error("/proc/acpi/call 不可用（acpi_call 内核模块没加载？）")]
    AcpiUnavailable,

    /// 底层文件读写失败
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),

    /// BIOS 返回码非 0（0x03=命令不可用，0x05=缓冲区太小）
    #[error("BIOS 返回码 {0} (0x{0:02X})")]
    BiosCode(u32),

    /// 返回数据比预期的短
    #[error("返回数据太短: 得到 {got} 字节，需要 {need}")]
    ShortResponse { got: usize, need: usize },

    /// BIOS 没有返回 "PASS" 成功标记
    #[error("BIOS 未返回 PASS（命令被拒绝）")]
    NotPass,

    /// 能力门控拒绝（本机型不支持该操作）
    #[error("本机型不支持该操作")]
    Unsupported,

    /// 参数越界
    #[error("参数越界: {value} 不在 [{min}, {max}] 内")]
    OutOfRange { value: u32, min: u32, max: u32 },
    #[error("JSON 序列化失败: {0}")]
    Json(#[from] serde_json::Error),

    /// 参数（十六进制/数字）解析失败
    #[error("参数解析失败: {0}")]
    Parse(#[from] std::num::ParseIntError),
}
