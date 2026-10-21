use std::fmt;

pub type Result<T> = std::result::Result<T, AiError>;

#[derive(Debug)]
pub enum AiError {
    Io(std::io::Error),
    Json(serde_json::Error),
    InvalidInput(String),
    MissingApiKey,
    Http { status: u16, message: String },
    Network(String),
    Protocol(String),
    Truncated { finish_reason: String },
    InvalidState(String),
    UnsupportedAction(String),
}
impl fmt::Display for AiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "文件读写失败：{e}"),
            Self::Json(e) => write!(f, "JSON 解析失败：{e}"),
            Self::InvalidInput(e)
            | Self::InvalidState(e)
            | Self::Protocol(e)
            | Self::Network(e) => f.write_str(e),
            Self::MissingApiKey => f.write_str("AI API Key 未配置"),
            Self::Http { status, message } => write!(f, "AI 服务返回 HTTP {status}：{message}"),
            Self::Truncated { finish_reason } => write!(
                f,
                "LLM 输出被截断（{finish_reason}），请增加 max_tokens 或缩短输入"
            ),
            Self::UnsupportedAction(e) => write!(f, "尚未支持的 AI 动作：{e}"),
        }
    }
}
impl std::error::Error for AiError {}
impl From<std::io::Error> for AiError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for AiError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
