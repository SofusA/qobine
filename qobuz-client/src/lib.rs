use snafu::prelude::*;

pub mod client;
pub mod qobuz_models;
pub mod stream;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to get a usable secret from Qobuz."))]
    ActiveSecret,
    #[snafu(display("Failed to get an app id from Qobuz."))]
    AppID,
    #[snafu(display("Failed to login."))]
    Login,
    #[snafu(display("Failed to create client"))]
    Create,
    #[snafu(display("{message}"))]
    Api { message: String },
    #[snafu(display("Failed to deserialize json: {message}"))]
    DeserializeJSON { message: String },
    #[snafu(display("Unable to start stream: {message}"))]
    StreamError { message: String },
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        let status = error.status();

        match status {
            Some(status) => Error::Api {
                message: status.to_string(),
            },
            None => Error::Api {
                message: "Unable to connect to Qobuz api".to_string(),
            },
        }
    }
}
