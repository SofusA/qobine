# macos-module

Registers qobine as the macOS now-playing application through the MediaPlayer framework.

Consumed by `tui-module`. The crate compiles to an empty library on other platforms.

The MediaPlayer framework delivers remote command events through the main thread run loop, so consumers must keep the main thread pumping `run_main_loop()` while the tokio runtime runs on another thread, and call `stop_main_loop()` on shutdown.
