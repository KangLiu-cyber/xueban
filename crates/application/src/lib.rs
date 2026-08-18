//! 应用层：用例编排（输入端口实现）。
//!
//! 服务依赖 `domain::ports` 中的输出端口 trait，由被驱动适配器
//! （adapter-postgres 等）实现并注入；测试用 `inmem` 内存替身。

pub mod agent;
pub mod attachments;
pub mod auth;
pub mod paper;
pub mod quiz;
pub mod space;
pub mod training;
pub mod wrong;

#[cfg(test)]
pub mod inmem;
