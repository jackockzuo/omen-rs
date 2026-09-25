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

/// 当前进程是否以 root 运行。
/// 读 /proc/self/status 而不是用 libc，遵守 crate 的 no-unsafe 约定；
/// 读不到（非 Linux）按非 root 处理。
pub fn is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .and_then(|l| l.split_whitespace().nth(1).map(|uid| uid == "0"))
        })
        .unwrap_or(false)
}

/// 读取类命令经 omend socket 执行，返回响应文本。
/// socket 不可用（omend 未运行）返回 None，调用方回退直连硬件。
pub fn query_via_daemon(name: &str, json: bool) -> Option<String> {
    send(&encode_query(name, json)).ok()
}

/// 把 (命令名, json) 编码成 socket 请求行。
pub fn encode_query(name: &str, json: bool) -> String {
    if json {
        format!("{name} --json")
    } else {
        name.to_string()
    }
}

/// 解析 socket 请求行，返回 (命令名, json)。
/// 协议：首个 token 是命令名；其余 token 出现 `--json` 即开 JSON 输出。
/// 多余 token（如写命令参数）被丢弃——socket 侧只能执行只读命令。
pub fn parse_query(line: &str) -> (&str, bool) {
    let mut tokens = line.split_whitespace();
    let name = tokens.next().unwrap_or("");
    let json = tokens.any(|t| t.eq_ignore_ascii_case("--json"));
    (name, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_plain_name() {
        assert_eq!(parse_query("fan"), ("fan", false));
    }

    #[test]
    fn parse_query_with_json_flag() {
        assert_eq!(parse_query("sensors --json"), ("sensors", true));
    }

    #[test]
    fn parse_query_tolerates_extra_whitespace() {
        assert_eq!(parse_query("  perf   --json  "), ("perf", true));
    }

    #[test]
    fn parse_query_empty_line() {
        assert_eq!(parse_query(""), ("", false));
    }

    #[test]
    fn parse_query_takes_first_token_only() {
        // 防线：即使 socket 收到写命令参数，也只按首个 token 派发只读命令
        assert_eq!(parse_query("fan set 30 --json"), ("fan", true));
        assert_eq!(parse_query("raw 08 02 2E 30 30"), ("raw", false));
    }

    #[test]
    fn encode_query_roundtrip() {
        assert_eq!(encode_query("fan", true), "fan --json");
        assert_eq!(encode_query("fan", false), "fan");
        assert_eq!(parse_query(&encode_query("info", true)), ("info", true));
        assert_eq!(parse_query(&encode_query("gpu", false)), ("gpu", false));
    }
}
