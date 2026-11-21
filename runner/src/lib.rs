pub mod progress;

#[derive(Debug)]
pub struct JobError {
    pub category: &'static str,
    pub message: String,
}

impl From<std::io::Error> for JobError {
    fn from(error: std::io::Error) -> Self {
        Self {
            category: "storage",
            message: error.to_string(),
        }
    }
}
