//! 响应解码器。
//!
//! 每个函数签名：`&[u8] -> Result<T, OmenError>`，对任意输入都不 panic。
use crate::error::OmenError;
use serde::Serialize;

/// `0x28` 系统设计数据（只取前 12 字节，其余无意义）。
///
/// 字段布局见 `docs/COMMAND-REFERENCE.md` §4。
#[derive(Debug, Clone, Serialize)]
pub struct SystemDesign {
    pub adapter_watts: u16,
    pub thermal_version: u8,
    pub sw_fan_control: bool,
    pub extreme_support: bool,
    pub extreme_unlock: bool,
    pub dt_bios_control: bool,
    pub two_byte_pl4: bool,
    pub pl4_default: u8,
    pub gpu_mode_switch: u8,
    pub cpu_limit_with_gpu_watts: u8,
    pub loadline_levels: u8,
    pub loadline_default: u8,
    pub ir_sensor: bool,
    pub pch_sensor: bool,
    pub vr_sensor: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuPower {
    pub ctgp: u8,
    pub ppab: u8,
    pub dstate: u8,
    pub gps: u8, // GPS 温度阈值(°C),本机出厂 87
}

#[derive(Debug, Clone, Serialize)]
/// 单个风扇。
pub struct Fan {
    pub level: u8, // 转速档(0x2D 的原始档位值)
                   // 将来可加: kind: FanKind(CPU/GPU,来自 0x2C)、rpm: Option<u16>
}
#[derive(Debug, Clone, Serialize)]
/// 整机风扇状态。
pub struct FanStatus {
    pub fans: Vec<Fan>, // 长度 == 数量(来自 0x10)
}
/// 0x0F 智能适配器。
#[derive(Debug, Clone, Serialize)]
pub struct SmartAdapter {
    pub status: u8,                   // [0]
    pub barrel_port: bool,            // [1] bit7
    pub usbc_design_watts: u16,       // [2] × 5
    pub connected_watts: Option<u16>, // [3] × 5；0xFF = 未知 → None
}

pub fn decode_smart_adapter(d: &[u8]) -> Result<SmartAdapter, OmenError> {
    if d.len() < 4 {
        return Err(OmenError::ShortResponse {
            got: d.len(),
            need: 4,
        });
    }

    Ok(SmartAdapter {
        status: d[0],
        barrel_port: d[1] & 0x80 != 0,
        usbc_design_watts: (d[2] as u16) * 5,
        connected_watts: if d[3] == 0xFF {
            None // 0xFF = 未知
        } else {
            Some((d[3] as u16) * 5) // 否则正常 ×5（同样先转 u16 防溢出）
        },
    })
}

impl SystemDesign {
    /// 适配器 ≥200W → 支持 BIOS 性能模式
    pub fn supports_bios_perf(&self) -> bool {
        self.adapter_watts >= 200
    }

    /// 适配器 ≥280W → 支持 TGP/PPAB
    pub fn supports_tgp_ppab(&self) -> bool {
        self.adapter_watts >= 280
    }
}

/// 解析 `0x28` 的返回数据。
pub fn decode_system_design(d: &[u8]) -> Result<SystemDesign, OmenError> {
    if d.len() < 12 {
        return Err(OmenError::ShortResponse {
            got: d.len(),
            need: 12,
        });
    }
    let f = d[4];
    Ok(SystemDesign {
        adapter_watts: u16::from_le_bytes([d[0], d[1]]),
        thermal_version: d[3],
        sw_fan_control: f & 0x01 != 0,
        extreme_support: f & 0x02 != 0,
        extreme_unlock: f & 0x04 != 0,
        dt_bios_control: f & 0x08 != 0,
        two_byte_pl4: f & 0x10 != 0,
        pl4_default: d[5],
        gpu_mode_switch: d[7],
        cpu_limit_with_gpu_watts: d[8],
        loadline_levels: d[9] & 0x0F,
        loadline_default: (d[9] >> 4) & 0x0F,
        ir_sensor: (d[10] & 0x03) == 0x02, // 两位组合，不是单读 bit0
        pch_sensor: d[10] & 0x04 != 0,
        vr_sensor: d[10] & 0x08 != 0,
    })
}

pub fn decode_gpu_power(d: &[u8]) -> Result<GpuPower, OmenError> {
    if d.len() < 4 {
        return Err(OmenError::ShortResponse {
            got: d.len(),
            need: 4,
        });
    }

    Ok(GpuPower {
        ctgp: d[0],
        ppab: d[1],
        dstate: d[2],
        gps: d[3],
    })
}

pub fn decode_fan_status(count_raw: &[u8], levels_raw: &[u8]) -> Result<FanStatus, OmenError> {
    if count_raw.is_empty() {
        // ← 加这 3 行：先守住索引
        return Err(OmenError::ShortResponse { got: 0, need: 1 });
    }
    let count = count_raw[0]; // 来自硬件，不可信！
    let n = (count as usize).min(levels_raw.len()); // ★ 夹紧：取 count 和实际长度的较小值
    let fans = (0..n)
        .map(|i| Fan {
            level: levels_raw[i],
        })
        .collect();
    Ok(FanStatus { fans })
}

#[cfg(test)]
mod tests {
    use super::*;
    macro_rules! never_panics {
        ($name:ident,$f:expr) => {
            proptest! {
                #[test]
                fn $name(data in proptest::collection::vec(any::<u8>(),0)){
                    let _ = $f(&data);
                }
            }
        };
    }

    use proptest::prelude::*;

    #[test]
    fn decodes_smart_adapter() {
        // 本机实测 01 00 00 00
        let a = decode_smart_adapter(&[0x01, 0x00, 0x00, 0x00]).unwrap();
        assert_eq!(a.status, 1);
        assert!(!a.barrel_port); // d[1]=0 → bit7=0 → false
        assert_eq!(a.usbc_design_watts, 0); // d[2]=0 → 0×5=0
        assert_eq!(a.connected_watts, Some(0)); // d[3]=0(不是0xFF) → Some(0×5)
    }

    #[test]
    fn smart_adapter_handles_sentinel() {
        // d[3]=0xFF → 未知 → None；d[1]=0x80 → bit7=1 → barrel=true
        let a = decode_smart_adapter(&[0x01, 0x80, 0x64, 0xFF]).unwrap();
        assert!(a.barrel_port); // 0x80 & 0x80 != 0
        assert_eq!(a.usbc_design_watts, 500); // 0x64=100 → 100×5=500
        assert_eq!(a.connected_watts, None); // 0xFF → 未知
    }

    #[test]
    fn fan_status_clamps_garbage_count() {
        // ★ 硬件抽风返回 count=200，但 levels 只有 4 字节 → 必须夹到 4，绝不 panic
        let s = decode_fan_status(&[200, 0, 0, 0], &[0x10, 0x20, 0x30, 0x40]).unwrap();
        assert_eq!(s.fans.len(), 4);
    }
    #[test]
    fn decodes_fan_status() {
        let count: [u8; 4] = [0x02, 0x00, 0x00, 0x00]; // 本机 2 个风扇
        let levels: [u8; 4] = [0x1b, 0x1e, 0x00, 0x00]; // 实测档位
        let s = decode_fan_status(&count, &levels).unwrap();
        assert_eq!(s.fans.len(), 2);
        assert_eq!(s.fans[0].level, 0x1b);
        assert_eq!(s.fans[1].level, 0x1e);
    }

    #[test]
    fn decodes_known_reply() {
        // 本机实测的 0x28 前 12 字节
        let raw = [
            0x18, 0x01, 0x32, 0x01, 0x01, 0xc8, 0x01, 0x0c, 0x37, 0x00, 0x00, 0x00,
        ];
        let sd = decode_system_design(&raw).unwrap();
        assert_eq!(sd.adapter_watts, 280);
        assert_eq!(sd.thermal_version, 1);
        assert!(sd.sw_fan_control);
        assert!(!sd.two_byte_pl4);
        assert_eq!(sd.pl4_default, 200);
    }

    #[test]
    fn decodes_gpu_power() {
        let raw = [0x01, 0x01, 0x01, 0x57]; // 本机实测返回
        let g = decode_gpu_power(&raw).unwrap();
        assert_eq!(g.gps, 87); // ← 这行才能抓住 “取错字节”

        assert_eq!(g.ctgp, 0x01);
        assert_eq!(g.ppab, 0x01);
        assert_eq!(g.dstate, 0x01);
    }
    #[test]
    fn gpu_power_rejects_short_input() {
        // 只给 3 字节,不够 4,必须返回 Err 而不是 panic
        assert!(decode_gpu_power(&[0x01, 0x01, 0x01]).is_err());
    }
    never_panics!(decode_system_never_panics, decode_system_design);
    never_panics!(decode_gpu_power_never_panics, decode_gpu_power);
    never_panics!(decode_smart_adapter_never_panics, decode_smart_adapter);

    proptest! {
        #[test]
        fn fan_status_never_panics(
            count  in proptest::collection::vec(any::<u8>(), 0..8),
            levels in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let _ = decode_fan_status(&count, &levels);   // 任意 count/levels 组合都不能崩
        }
    }
}
