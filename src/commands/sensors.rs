//! 传感器温度。0x23 {idx,0,0,0} -> 4B
//!   idx: 0=IR 1=Ambient 2=PCH 3=VR

use serde::Serialize;

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
