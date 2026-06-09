use qobuz_player_controls::{AppResult, StatusReceiver, error::Error};
use rppal::gpio::Gpio;
use tokio::sync::watch;

const GPIO: u8 = 23;

pub async fn init(
    mut status_receiver: StatusReceiver,
    active_receiver: watch::Receiver<bool>,
) -> AppResult<()> {
    let mut pin = Gpio::new()
        .or(Err(Error::GpioUnavailable { pin: GPIO }))?
        .get(GPIO)
        .or(Err(Error::GpioUnavailable { pin: GPIO }))?
        .into_output();
    tracing::info!("Pin claimed");

    loop {
        if status_receiver.changed().await.is_ok() {
            let status = status_receiver.borrow_and_update();
            let is_active = *active_receiver.borrow();
            if !is_active {
                continue;
            }

            match *status {
                qobuz_player_controls::Status::Paused => {
                    pin.set_low();
                    tracing::info!("Gpio low");
                }
                qobuz_player_controls::Status::Playing
                | qobuz_player_controls::Status::Buffering => {
                    pin.set_high();
                    tracing::info!("Gpio high");
                }
            }
        }
    }
}
