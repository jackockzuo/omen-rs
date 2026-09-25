//! 能力探测与写操作门控。
//!
//! 启动时读 0x28 得到 SystemDesignData，缓存成能力快照；
//! 所有写操作先查这里，不支持则返回 OmenError::Unsupported。
//!
//! 待实现：
//!   struct Capabilities { ... }         // 见 docs/COMMAND-REFERENCE.md §4
//!   fn probe() -> Result<Capabilities, OmenError>
//!   fn ensure(&self, cap: CapFlag) -> Result<(), OmenError>

use bitflags::bitflags;

use crate::{
    error::OmenError,
    protocol::{
        command::{CommandType, CMD_PERF},
        decode::{decode_system_design, SystemDesign},
    },
    transport,
};

use std::sync::{Arc, OnceLock};

static CAPS: OnceLock<Arc<Capabilities>> = OnceLock::new();

pub fn caps() -> Result<&'static Arc<Capabilities>, OmenError> {
    if let Some(cached) = CAPS.get() {
        return Ok(cached);
    }
    let probed = Capabilities::probe()?;
    let arc = Arc::new(probed);

    // 即使多线程并发导致 set 失败（Err），我们也无所谓，直接忽略
    let _ = CAPS.set(arc);

    // 用 ok_or 替代 unwrap，满足 clippy 的 deny 规则
    CAPS.get().ok_or(OmenError::NotPass)
}
bitflags! {
    /// 本机能力位（从 0x28 的 SystemDesign 推导）。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Cap: u32 {
        const SW_FAN_CONTROL  = 1 << 0;   // 软件风扇控制（写风扇的前提）
        const BIOS_PERF       = 1 << 1;   // 适配器 ≥200W
        const TGP_PPAB        = 1 << 2;   // 适配器 ≥280W（写 GPU 功率的前提）
        const EXTREME_SUPPORT = 1 << 3;
        const EXTREME_UNLOCK  = 1 << 4;
    }
}

/// 纯逻辑：从 0x28 数据推导能力位（可单测）。
fn caps_from_design(d: &SystemDesign) -> Cap {
    let mut caps = Cap::empty();
    if d.sw_fan_control {
        caps |= Cap::SW_FAN_CONTROL;
    }
    if d.supports_bios_perf() {
        caps |= Cap::BIOS_PERF;
    }
    if d.supports_tgp_ppab() {
        caps |= Cap::TGP_PPAB;
    }
    if d.extreme_support {
        caps |= Cap::EXTREME_SUPPORT;
    }
    if d.extreme_unlock {
        caps |= Cap::EXTREME_UNLOCK;
    }
    caps
}

pub struct Capabilities {
    pub design: SystemDesign, // 完整 0x28 数据
    pub caps: Cap,            // 提炼的能力位
}

impl Capabilities {
    /// 读 0x28 → 能力快照（启动时调一次，缓存复用）。
    pub fn probe() -> Result<Self, OmenError> {
        let design =
            decode_system_design(&transport::read(CMD_PERF, CommandType::SysDesign, 128)?)?;
        let caps = caps_from_design(&design);
        Ok(Self { design, caps })
    }

    /// ★ 写操作门控：要求的能力不具备就拒绝。
    pub fn ensure(&self, required: Cap) -> Result<(), OmenError> {
        if self.caps.contains(required) {
            Ok(())
        } else {
            Err(OmenError::Unsupported) // 复用已有错误
        }
    }
}
