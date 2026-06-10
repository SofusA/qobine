# Qobuz RFID player

The RFID player allows users to put on music using RFID tags and an RFID scanner. 
![rfid player](/assets/rfid-player.gif?raw=true)

## Hardware
- Speaker system with amplifier 
- Raspberry pi 4 or newer
- DAC to go from raspberry pi to amplifier 
- a lot of RFID-tags ([Link to aliexpress](https://www.aliexpress.com/item/32814647380.html))
- usb rfid scanner ([Link to aliexpress](https://www.aliexpress.com/item/32790407197.html))
- Optionally plastic sleeves ([Link to aliexpress](https://www.aliexpress.com/item/1005007369596457.html))

## Album cards
This tool can be used to print album cards: [sofusa/album-card-generator](https://github.com/SofusA/album-card-generator)

## Usage
Can be run as a standalone player, or enabled in web, rfid and tui with the `--rfid` flag.

### Linking tags with albums or playlists
Click the "Link" button on album or playlist pages, and then scan an RFID tag. Afterwards whenever this tag is scanned, the given album or playlist will be played.

<img src="/assets/link-album.png?raw=true" width="240">

### Using another player as RFID database
You can share the RFID database with multiple players by setting a base address and an optional secret:
```
--rfid-server-base-address <RFID_SERVER_BASE_ADDRESS>
    Use other qobuz-player with web for rfid database
--rfid-server-secret <RFID_SERVER_SECRET>
    Secret for optional qobuz-player rfid server
```
