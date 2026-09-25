//! EC（嵌入式控制器）RAM 直接读写。
//!
//! 通过 ec_sys 内核模块的 debugfs 接口（/sys/kernel/debug/ec/ec0/io），
//! 直接读写 EC RAM 偏移量。需要 root + ec_sys write_support=1。
//!
//! 与 transport 层的 WMAA（ACPI 方法调用）是两条完全不同的路径：
//! - WMAA：请求固件代为执行，固件解释后可能写 EC
//! - EC RAM：跳过固件，直接读写 EC 微控制器的内存

use crate::error::OmenError;
use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const EC_IO_PATH: &str = "/sys/kernel/debug/ec/ec0/io";

fn ec_file() -> Result<std::fs::File, OmenError> {
    if !Path::new(EC_IO_PATH).exists() {
        return Err(OmenError::AcpiUnavailable);
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(EC_IO_PATH)
        .map_err(Into::into)
}

pub fn read(offset: u16) -> Result<u8, OmenError> {
    let mut f = ec_file()?;
    f.seek(SeekFrom::Start(offset as u64))?;
    let mut buf = [0u8; 1];
    f.read_exact(&mut buf)?;
    Ok(buf[0])
}

pub fn write(offset: u16, value: u8) -> Result<(), OmenError> {
    let mut f = ec_file()?;
    f.seek(SeekFrom::Start(offset as u64))?;
    f.write_all(&[value])?;
    Ok(())
}

/// 写入后立刻读回验证，失败则返回错误。
pub fn write_verified(offset: u16, value: u8) -> Result<(), OmenError> {
    write(offset, value)?;
    let readback = read(offset)?;
    if readback != value {
        return Err(OmenError::BiosCode(readback as u32));
    }
    Ok(())
}
