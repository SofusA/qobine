# qobuz-player

## High resolution audio player backed by Qobuz

Powered by [Qobuz](https://www.qobuz.com). Requires a paid subscription. This does not allow you to listen for free.

This is a mono repo for multiple third party apps for Qobuz.

This includes a terminal app, a web server and web-ui, a RFID player, and a minimal Qobuz Connect player. 

The web interface is ideal for a setup with a single board computer, e.g. Raspberry Pi, connected to the speaker system and controlled with a smartphone or tablet.

### Terminal UI
![TUI Screenshot](/assets/qobuz-player.png)

#### Keyboard Shortcuts
Press <kbd>?</kbd> for an overview of all available keyboard shortcuts

### GNOME player
<img src="/assets/qobuz-player-gtk.png?raw=true">

### Web UI
<img src="/assets/qobuz-player-webui.jpg?raw=true" width="240">

### RFID player
![RFID player](/assets/rfid-player.gif?raw=true)

Read more [in the wiki](https://github.com/SofusA/qobuz-player/wiki/RFID-player)

## Player Features

- High resolution audio: Supports up to 24bit/192Khz (max quality Qobuz offers)
- MPRIS support (control via [playerctl](https://github.com/altdesktop/playerctl) or other D-Bus client)
- Gap-less playback
- Experimental Qobuz Connect. Enabled with `--connect` flag. Optionally set the session manager port with `--connect-port` (defaults to a random port).

### Build from source

Linux dependencies: `alsa-sys-devel`, `just`.
```
cargo build
```

## Development
1. Setup sqlx: `just create-env-file`. Only needed once. 
2. Init sqlite database: `init-database`.
3. For webui development in `qobuz-player-web`:
  - `npm i`. Install npm dependencies. 
  - `npm run watch`. Watch for style changes. 

## Get started
Install your favorites app.

Run `qobuz-player --help` or `qobuz-player <subcommand> --help` to see all available options.

## Web UI

The player can start an embedded web interface. This is disabled by default and must be started with the `--web` argument. It also listens on `0.0.0.0:9888` by default. Change port with `--port` argument.

Go to `http://localhost:9888` to view the UI.

## Contribution
Feature requests, issues and contributions are very welcome.

## Credits
Qobuz-player started as a fork of [hifi.rs](https://github.com/iamdb/hifi.rs) but has since diverged. 
