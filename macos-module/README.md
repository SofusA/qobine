# macos-module

Registers qobine as the macOS now-playing application through the MediaPlayer framework.

Consumed by `tui-module`. The crate compiles to an empty library on other platforms.

The MediaPlayer framework delivers remote command events through the main thread run loop, so consumers must keep the main thread pumping `MainLoop::run` while the tokio runtime runs on another thread, and stop it on shutdown through the paired `MainLoopStopper`.

Building on macOS requires the Swift toolchain: the `mediaplayer` dependency compiles a bundled Swift package in its build script.
