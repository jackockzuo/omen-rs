//! raw —— 原始 ACPI 命令探测（逃生口）。
//! 直接发任意 (command, ctype, mode, data)，打印固件返回的原始字节。

use crate::error::OmenError;
use crate::transport;

fn hex_u32(s: &str) -> Result<u32, OmenError> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    Ok(u32::from_str_radix(s, 16)?)
}

fn hex_u8(s: &str) -> Result<u8, OmenError> {
    let s = s.trim_start_matches("0x").trim_start_matches("0X");
    Ok(u8::from_str_radix(s, 16)?)
}

fn expected_for_mode(mode: u8) -> Result<usize, OmenError> {
    match mode {
        1 => Ok(0),
        2 => Ok(4),
        3 => Ok(128),
        4 => Ok(1024),
        5 => Ok(4096),
        _ => Err(OmenError::OutOfRange {
            value: mode as u32,
            min: 1,
            max: 5,
        }),
    }
}

pub fn run(command: &str, ctype: &str, mode: u8, data: &[String]) -> Result<(), OmenError> {
    let command = hex_u32(command)?;
    let ctype = hex_u8(ctype)?;
    let expected = expected_for_mode(mode)?;
    let data: Vec<u8> = data
        .iter()
        .map(|s| hex_u8(s))
        .collect::<Result<Vec<u8>, _>>()?;

    let resp = transport::wmaa_raw(command, ctype, &data, expected)?;

    println!("PASS — 返回 {} 字节:", resp.len());
    for (i, chunk) in resp.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        println!("{:04x}  {}", i * 16, hex.join(" "));
    }
    Ok(())
}
