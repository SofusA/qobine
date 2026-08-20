#[cfg(target_os = "macos")]
mod now_playing;

#[cfg(target_os = "macos")]
pub use now_playing::{run_with_main_loop, spawn_now_playing};
