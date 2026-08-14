use std::fmt;

/// 领域错误：应用层据此翻译为协议错误，驱动适配器据此映射 HTTP 状态码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// 资源不存在（账号、笔记、题目等）。
    NotFound(String),
    /// 与既有状态冲突（账号已注册、token 已吊销等）。
    Conflict(String),
    /// 业务校验失败（密码强度不足、防环被违反等）。
    Invalid(String),
    /// 存储层或其他基础设施失败。
    Storage(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(msg) => write!(f, "not found: {msg}"),
            Error::Conflict(msg) => write!(f, "conflict: {msg}"),
            Error::Invalid(msg) => write!(f, "invalid: {msg}"),
            Error::Storage(msg) => write!(f, "storage: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
