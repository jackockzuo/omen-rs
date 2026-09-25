//! 面向功能的命令封装：把 protocol + transport 组合成完整操作。
//!
//! 所有写操作流程固定：
//!   能力门控 -> 范围校验 -> 写 -> (可选)回读校验
use crate::error::OmenError;
pub mod adapter;
pub mod battery;
pub mod fan;
pub mod gpu;
pub mod info;
pub mod perf;
pub mod power;
pub mod raw;
pub mod sensors;
pub mod thermal;

pub trait Command: Send + Sync {
    fn name(&self) -> &'static str;
    fn run(&self, json: bool) -> Result<String, OmenError>;
    fn boxed_clone(&self) -> Box<dyn Command>;
}

pub fn registry() -> Vec<Box<dyn Command>> {
    vec![
        Box::new(adapter::AdapterCommand),
        Box::new(fan::FanCommand),
        Box::new(gpu::GpuCommand),
        Box::new(info::InfoCommand),
        Box::new(perf::PerfCommand),
        Box::new(sensors::SensorsCommand),
    ]
}
