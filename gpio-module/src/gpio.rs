use controls_module::{Status, StatusReceiver};
use player_module::{AppResult, error::Error};
use rppal::gpio::Gpio;
use tokio::sync::watch;

const GPIO: u8 = 23;

pub async fn init(
    mut status_receiver: StatusReceiver,
    mut active_receiver: watch::Receiver<bool>,
) -> AppResult<()> {
    let mut pin = Gpio::new()
        .or(Err(Error::GpioUnavailable { pin: GPIO }))?
        .get(GPIO)
        .or(Err(Error::GpioUnavailable { pin: GPIO }))?
        .into_output();
    tracing::info!("Pin claimed");

    loop {
        tokio::select! {
            result = status_receiver.changed() => {
                if result.is_err() {
                    break;
                }

                update_gpio(
                    &mut pin,
                    *status_receiver.borrow_and_update(),
                    *active_receiver.borrow(),
                );
            }

            result = active_receiver.changed() => {
                if result.is_err() {
                    break;
                }

                update_gpio(
                    &mut pin,
                    *status_receiver.borrow(),
                    *active_receiver.borrow_and_update(),
                );
            }
        }
    }

    Ok(())
}

fn update_gpio(pin: &mut rppal::gpio::OutputPin, status: Status, is_active: bool) {
    if !is_active {
        pin.set_low();
        tracing::info!("Gpio low");
        return;
    }

    match status {
        Status::Paused => {
            pin.set_low();
            tracing::info!("Gpio low");
        }
        Status::Playing | Status::Buffering => {
            pin.set_high();
            tracing::info!("Gpio high");
        }
    }
}
