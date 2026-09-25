//! omend 客户端：通过 Unix socket 连接本机守护进程。

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

const SOCKET_PATH: &str = "/tmp/omend.sock";

/// 发送一条命令给 omend，返回 omend 的响应文本。
/// 连不上返回 Err（调用方决定如何提示用户）。
pub fn send(cmd: &str) -> Result<String, std::io::Error> {
    let mut stream = UnixStream::connect(SOCKET_PATH)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(cmd.as_bytes())?;
    let mut buf = String::new();
    stream.read_to_string(&mut buf)?;
    Ok(buf)
}
