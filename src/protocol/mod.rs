//! 纯协议层：无 I/O、无 unsafe，全部可单元测试。
//!
//! 子模块：
//!   secu    —— SECU 头拼装 + ACPI 参数串格式化 / 返回文本解析
//!   command —— Command 空间常量 + CommandType 枚举
//!   decode  —— 响应解码器

pub mod command;
pub mod decode;
pub mod secu;
