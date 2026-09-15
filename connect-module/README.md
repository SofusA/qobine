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

## Limitations

Loop and shuffle modes are not supported. qobine does not control other renderers yet.
