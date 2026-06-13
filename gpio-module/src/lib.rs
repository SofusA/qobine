#[cfg(feature = "gpio")]
mod gpio;

#[cfg(feature = "gpio")]
pub use gpio::init;
