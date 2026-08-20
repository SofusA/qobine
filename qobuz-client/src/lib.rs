use snafu::prelude::*;

pub mod client;
pub mod qobuz_models;
pub mod stream;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(display("Failed to get a usable secret from Qobuz."))]
    ActiveSecret,
    #[snafu(display("Internal error: Secret not set."))]
    SecretNotSet,
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
    Stream { message: String },
    #[snafu(display("{message}"))]
    Time { message: String },
    #[snafu(display("{message}"))]
    NumberConversion { message: String },
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        let status = error.status();

        status.map_or_else(
            || Self::Api {
                message: "Unable to connect to Qobuz api".to_string(),
            },
            |status| Self::Api {
                message: status.to_string(),
            },
        )
    }
}

impl From<reqwest::header::InvalidHeaderValue> for Error {
    fn from(error: reqwest::header::InvalidHeaderValue) -> Self {
        Self::NumberConversion {
            message: error.to_string(),
        }
    }
}

impl From<std::num::TryFromIntError> for Error {
    fn from(error: std::num::TryFromIntError) -> Self {
        Self::NumberConversion {
            message: error.to_string(),
        }
    }
}
