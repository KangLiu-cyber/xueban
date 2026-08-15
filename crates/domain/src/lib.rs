//! 学伴领域层：实体、值对象、领域服务与业务不变式，以及输出端口 traits。
//!
//! 零框架依赖（仅 serde/chrono 纯函数库），不知道 HTTP、MCP、SQL 的存在。

pub mod error;
pub mod event;
pub mod identity;
pub mod ports;
pub mod practice;
pub mod skill;
pub mod space;

pub use error::{Error, Result};
