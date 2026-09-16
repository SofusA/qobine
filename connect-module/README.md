# Connect support

Qobuz Connect support through the `qobuz-connect` crate. qobine joins the account's Connect session as a renderer, so the Qobuz apps list it as a playback device.

Can be run as a standalone player, or enabled in web, rfid and tui with the `--connect` flag, which exists only when the frontend is built with its `connect` cargo feature. The device name comes from `--connect-name`.

```
cargo run -p connect-module -- --connect-name qobine
cargo run -p tui-module --features connect -- --connect --connect-name qobine
```

## What works

- Playback control from the apps: play, pause, seek, skip to a track, volume, mute, maximum audio quality.
- The queue loaded or edited in an app replaces or updates qobine's queue without interrupting the current track.
- Queue edits made in qobine (add or insert tracks, remove, reorder) reach the apps. Starting a new queue in qobine makes it the active device, so playback moves to qobine.
- The playback device can be switched from the TUI: `c` lists the devices of the Connect session next to the disconnect players, Enter makes the selected one active. The server then hands it the current track and position.

## Limitations

Loop, shuffle and autoplay modes are not supported: qobine plays the queue as listed and stops at its end. Tracks the catalog no longer serves are skipped with a warning and stay only in the apps' queue. qobine does not control other renderers beyond making one active.

Choosing another device in an app pauses qobine, clearing the queue from an app stops the playing track, and a queue loaded from an app starts playing only when qobine is the selected device. Starting a track in qobine while another device is selected makes qobine the selected device, as the web player does; play and pause do not, so resuming a paused qobine plays alongside the selected device.
