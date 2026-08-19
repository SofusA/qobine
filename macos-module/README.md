# macos-module

Registers qobine as the macOS now-playing application through the MediaPlayer framework.

Consumed by `tui-module`. The crate compiles to an empty library on other platforms.

The MediaPlayer framework delivers remote command events through the main thread run loop, so binaries wrap their async entry point in `run_with_main_loop`: it runs the tokio runtime on a separate thread while the main thread pumps the run loop, and tears the loop down when the entry point returns.

Building on macOS requires the Swift toolchain: the `mediaplayer` dependency compiles a bundled Swift package in its build script.
