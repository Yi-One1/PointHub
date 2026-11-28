use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("凭据无效或已过期：{0}")]
    Auth(String),

    #[error("账号不参与该活动：{0}")]
    NotEligible(String),

    #[error("平台接口返回异常：{0}")]
    Upstream(String),

    #[error("网络请求失败：{0}")]
    Network(String),

    #[error("数据读写失败：{0}")]
    Io(String),

    #[error("数据格式不符：{0}")]
    Format(String),

    #[error("找不到对象：{0}")]
    NotFound(String),

    #[error("配置或输入有误：{0}")]
    Invalid(String),

    #[error("当前平台不支持该能力：{0}")]
    Unsupported(String),

    /// 本机登录态已找到，但读取它需要用户授权一次额外的本地操作。
    /// 界面据此显示「允许一次」按钮，而不是把它当成普通失败。
    #[error("{0}")]
    NeedsKey(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Format(e.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Error::Network(format!("请求超时：{e}"))
        } else if e.is_connect() {
            Error::Network(format!("连接失败：{e}"))
        } else {
            Error::Network(e.to_string())
        }
    }
}

impl Error {
    /// 是否值得重试。凭据失效与「无资格」重试也没用。
    pub fn retryable(&self) -> bool {
        matches!(self, Error::Network(_) | Error::Upstream(_))
    }

    /// 给用户看的一句话，不含内部堆栈。
    pub fn user_message(&self) -> String {
        match self {
            Error::Auth(m) => format!("凭据失效，请重新导入。{m}"),
            Error::NotEligible(m) => format!("该账号当前不参与此活动。{m}"),
            Error::Network(m) => format!("网络不通，请检查网络或代理。{m}"),
            other => other.to_string(),
        }
    }
}
