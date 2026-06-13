# qobuz-player / qobine

## High resolution audio player backed by Qobuz

Powered by [Qobuz](https://www.qobuz.com). Requires a paid subscription. This does not allow you to listen for free.

This is a mono repo for multiple third party apps for Qobuz.

This includes a terminal app, a web server and web-ui, a RFID player, and a minimal Qobuz Connect player. 

The web interface is ideal for a setup with a single board computer, e.g. Raspberry Pi, connected to the speaker system and controlled with a smartphone or tablet.

### Terminal UI
![TUI screenshot](/assets/qobuz-player.png)
[More info](/tui-module)

### GNOME player
![GTK screenshot](/assets/gtk-module.png)
[More info](/tui-module)

### Web UI
<img src="/assets/web-moduleui.jpg?raw=true" width="240">
[More info](/web-module)

### RFID player
![RFID player](/assets/rfid-player.gif?raw=true)
[More info](/rfid-module)

## Connect
### Qobuz Connect
There is initial support for Qobuz Connect in web, tui and rfid players, and a standalone minimal connect player.
This can be enabled with the `--connect` flag.
However this is currently very limited, experimental and full of bugs.
[More info on Connect player](/connect-module)

### Qobine Disconnect
The web player and the rfid player has support for custom self-hostable connect service, `Disconnect`, which allows activating and deactivating multiple running players.
This does **not** work with the official qobuz smartphone app or web player.

This can be used if you are running multiple players in your home, but would like to control all players centrally on your smartphone through one web player instance.

[More info on Disconnect](/disconnect-module)

### Raspberry pi
Web, rfid and connect connect players, support setting `GPIO 23` high when music is playing.
This is hidden behind a feature `GPIO`. Prebuild binaries for aarm64 linux has this enable.

Enable with `--gpio`.

This can be used to control power to amplifier when music is playing.


### Build from source
Linux dependencies: `alsa-sys-devel`.
```
cargo build
```

## Development
Linux dependencies: `alsa-sys-devel`, `just`, `sqlx-cli`, `npm`.
1. Setup sqlx: `just create-env-file`. Only needed once. 
2. Init sqlite database: `init-database`.
3. Additional for webui development: In `web-module`:
  - `npm i`. Install npm dependencies. 
  - `npm run watch`. Watch for style changes. 

## Get started
Install your favorites app.

Run `qobuz-player --help` or `qobuz-player <subcommand> --help` to see all available options.

## Contribution
Feature requests, issues and contributions are very welcome.

## Credits
Qobuz-player started as a fork of [hifi.rs](https://github.com/iamdb/hifi.rs) but has since diverged. 
