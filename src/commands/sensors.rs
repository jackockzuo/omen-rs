//! 传感器温度。0x23 {idx,0,0,0} -> 4B
//!   idx: 0=IR 1=Ambient 2=PCH 3=VR
//!
//! 注意：WMAA 0x23 的四个读数里没有 CPU 结温，且 PCH 常年 60-70°C（本机实测
//! 空闲即 63°C），**不能拿 max 驱动风扇曲线**——那会把风扇钉在高转速。
//! 曲线控制请用 [`cpu_temp`]（hwmon coretemp/k10temp，真正的 CPU 结温）。

use serde::Serialize;
use std::fs;
use std::path::Path;

use crate::error::OmenError;
use crate::protocol::command::{CommandType, CMD_PERF};

use crate::transport;

#[derive(Serialize)]
pub struct Sensor {
    #[serde(rename = "ir")]
    idx0: Option<u8>,
    #[serde(rename = "ambient")]
    idx1: Option<u8>,
    #[serde(rename = "pch")]
    idx2: Option<u8>,
    #[serde(rename = "vr")]
    idx3: Option<u8>,
}
/// 读取传感器温度。
pub fn get() -> Sensor {
    let read_idx = |i: u8| {
        transport::wmaa(CMD_PERF, CommandType::Temp, &[i, 0, 0, 0], 4)
            .ok()
            .and_then(|d| d.first().copied())
    };

    let vals: Vec<Option<u8>> = [0, 1, 2, 3].iter().map(|&i| read_idx(i)).collect();

    Sensor {
        idx0: vals[0],
        idx1: vals[1],
        idx2: vals[2],
        idx3: vals[3],
    }
}

impl Sensor {
    pub fn ir(&self) -> Option<u8> {
        self.idx0
    }

    pub fn max_temp(&self) -> Option<u8> {
        [self.idx0, self.idx1, self.idx2, self.idx3]
            .iter()
            .copied()
            .flatten()
            .max()
    }
}

/// CPU 结温（°C），从 hwmon 读取。
///
/// hwmon 驱动目录名：`coretemp`（Intel）/ `k10temp`、`zenpower`（AMD）。
/// 这是风扇曲线的正确输入：PCH/VR 即使空闲也常在 60°C+，不反映散热需求。
pub fn cpu_temp() -> Option<u8> {
    cpu_temp_from(Path::new("/sys/class/hwmon"))
}

/// `root` 注入版（单测用临时目录代替 /sys/class/hwmon）。
///
/// 任一 hwmon 目录读失败只跳过该目录；整体读不到才返回 None。
/// 温度合理性范围 (0, 200) °C，范围外的读数直接忽略。
pub fn cpu_temp_from(root: &Path) -> Option<u8> {
    let mut best: Option<u8> = None;
    for hwmon in fs::read_dir(root).ok()?.flatten() {
        let name = fs::read_to_string(hwmon.path().join("name")).unwrap_or_default();
        if !matches!(name.trim(), "coretemp" | "k10temp" | "zenpower") {
            continue;
        }
        let Ok(entries) = fs::read_dir(hwmon.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let Some(fname) = fname.to_str() else {
                continue;
            };
            if !(fname.starts_with("temp") && fname.ends_with("_input")) {
                continue;
            }
            let Ok(text) = fs::read_to_string(entry.path()) else {
                continue;
            };
            let Ok(milli) = text.trim().parse::<i32>() else {
                continue;
            };
            if !(0 < milli && milli < 200_000) {
                continue;
            }
            best = best.max(Some((milli / 1000) as u8));
        }
    }
    best
}

/// 把 Option<温度> 格式化:有值 → “NN °C”,没值 → “--”。
fn celsius(t: Option<u8>) -> String {
    match t {
        Some(v) => format!("{v} °C"),
        None => "--".to_string(),
    }
}
pub fn format(json: bool) -> Result<String, OmenError> {
    let s = get();
    if json {
        Ok(serde_json::to_string_pretty(&s)?)
    } else {
        let labels = ["IR", "Ambient", "PCH", "VR"];
        let values = [s.idx0, s.idx1, s.idx2, s.idx3];
        let mut out = String::new();
        for (label, val) in labels.iter().zip(values) {
            out.push_str(&format!("{label:<10}: {}\n", celsius(val)));
        }
        Ok(out.trim_end().to_string())
    }
}

pub fn print(json: bool) -> Result<(), OmenError> {
    println!("{}", format(json)?);
    Ok(())
}

pub struct SensorsCommand;
impl super::Command for SensorsCommand {
    fn name(&self) -> &'static str {
        "sensors"
    }
    fn run(&self, json: bool) -> Result<String, OmenError> {
        format(json)
    }
    fn boxed_clone(&self) -> Box<dyn super::Command> {
        Box::new(Self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// 在临时目录里伪造 hwmon 结构：
    /// root/hwmonN/name = driver, root/hwmonN/tempX_input = 毫度
    fn fake_hwmon(root: &Path, hwmon: &str, driver: &str, temps: &[(&str, i32)]) {
        let dir = root.join(hwmon);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("name"), driver).unwrap();
        for (file, milli) in temps {
            fs::write(dir.join(file), format!("{milli}\n")).unwrap();
        }
    }

    #[test]
    fn cpu_temp_reads_coretemp_max() {
        let root = std::env::temp_dir().join("omen-test-coretemp");
        let _ = fs::remove_dir_all(&root);
        fake_hwmon(&root, "hwmon0", "nvme", &[("temp1_input", 45_000)]);
        fake_hwmon(
            &root,
            "hwmon8",
            "coretemp",
            &[("temp1_input", 48_000), ("temp2_input", 55_000)],
        );
        assert_eq!(cpu_temp_from(&root), Some(55));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cpu_temp_accepts_amd_drivers() {
        let root = std::env::temp_dir().join("omen-test-k10temp");
        let _ = fs::remove_dir_all(&root);
        fake_hwmon(&root, "hwmon0", "k10temp", &[("temp1_input", 61_000)]);
        assert_eq!(cpu_temp_from(&root), Some(61));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cpu_temp_ignores_non_cpu_hwmon() {
        let root = std::env::temp_dir().join("omen-test-noncpu");
        let _ = fs::remove_dir_all(&root);
        // nvme 55°C、acpitz 70°C：都不是 CPU，必须被忽略（PCH 陷阱的防线）
        fake_hwmon(&root, "hwmon0", "nvme", &[("temp1_input", 55_000)]);
        fake_hwmon(&root, "hwmon1", "acpitz", &[("temp1_input", 70_000)]);
        assert_eq!(cpu_temp_from(&root), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cpu_temp_rejects_implausible_readings() {
        let root = std::env::temp_dir().join("omen-test-badread");
        let _ = fs::remove_dir_all(&root);
        fake_hwmon(
            &root,
            "hwmon0",
            "coretemp",
            &[
                ("temp1_input", 0),          // 零值无效
                ("temp2_input", -12_000),    // 负数无效
                ("temp3_input", 250_000),    // 超出物理范围
                ("temp4_input", 62_000),     // 唯一合理值
            ],
        );
        assert_eq!(cpu_temp_from(&root), Some(62));
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cpu_temp_missing_dir_is_none() {
        assert_eq!(
            cpu_temp_from(Path::new("/nonexistent/hwmon-omen-test")),
            None
        );
    }

    #[test]
    fn cpu_temp_tolerates_broken_hwmon_dir() {
        let root = std::env::temp_dir().join("omen-test-brokendir");
        let _ = fs::remove_dir_all(&root);
        fake_hwmon(&root, "hwmon0", "coretemp", &[]); // 无 temp*_input
        fs::remove_file(root.join("hwmon0/name")).unwrap(); // name 也没了
        assert_eq!(cpu_temp_from(&root), None);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    #[ignore = "硬件在环：只在装有 coretemp/k10temp 的真机上有效，CI 跳过"]
    fn cpu_temp_on_this_machine() {
        let t = cpu_temp();
        println!("真机 cpu_temp = {t:?} °C");
        assert!(t.is_some(), "真机应有 CPU hwmon（coretemp/k10temp）");
        assert!((1..=150).contains(&t.unwrap()), "读数应在物理范围内: {t:?}");
    }
}
