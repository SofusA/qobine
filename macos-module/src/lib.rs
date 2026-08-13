#[cfg(target_os = "macos")]
mod now_playing;

#[cfg(target_os = "macos")]
pub use now_playing::{MainLoop, MainLoopStopper, spawn_now_playing};
