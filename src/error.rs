#[derive(Clone)]
pub struct AppError {
    exit_code: u8,
    message: String,
}

impl AppError {
    pub fn new(exit_code: u8, message: impl Into<String>) -> Self {
        Self {
            exit_code,
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> u8 {
        self.exit_code
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::fmt::Debug for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppError")
            .field("exit_code", &self.exit_code)
            .field("message", &self.message)
            .finish()
    }
}

impl std::error::Error for AppError {}

