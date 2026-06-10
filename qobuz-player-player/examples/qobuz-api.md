# Qobuz API Reference

Comprehensive reverse-engineered documentation of the Qobuz API, derived from the web player
(`play.qobuz.com`) bundle.js and network traffic analysis.

## Overview

- **Base URL**: `https://www.qobuz.com/api.json/0.2/`
- **Protocol**: HTTPS REST (JSON responses)
- **Auth**: Custom header-based auth + MD5 request signing
- **Streaming**: Segmented CMAF/fMP4 with AES-128-CTR encryption (qbz-1 profile)

All endpoints follow the pattern:
```
{METHOD} https://www.qobuz.com/api.json/0.2/{object}/{method}?{params}
```

---

## Authentication

### Headers

Every authenticated request must include:

| Header | Value | Required |
|--------|-------|----------|
| `X-App-Id` | Application ID (e.g. `798273057`) | Always |
| `X-User-Auth-Token` | User auth token from login | After login |
| `X-Session-Id` | Session ID from `session/start` | Streaming only |

### App ID Extraction

The `app_id` is not a static value — it must be extracted from the web player at runtime:

1. Fetch `https://play.qobuz.com/login`
2. Parse the HTML for the bundle.js script tag: `<script src="/resources/{version}/bundle.js">`
3. Fetch the bundle.js
4. Regex extract from: `production:{api:{appId:"(\d{9})",appSecret:"(\w{32})"`

Current production values (may change with bundle updates):
- `appId`: `798273057`
- `appSecret`: `05a4851e74ee47fda346f50cfdfc4f09`

### Request Signing

Signed requests append `request_ts` and `request_sig` as query parameters.

**Algorithm**:
1. `request_ts` = current Unix timestamp (seconds)
2. Concatenate: `{object}{method}{sorted_key1}{value1}{sorted_key2}{value2}...{timestamp}{secret}`
   - Keys are sorted alphabetically
   - `{object}` and `{method}` are derived from the endpoint (e.g. `file/url` → object=`file`, method=`url` → concatenated as `fileurl`)
3. `request_sig` = MD5 hex digest of the concatenated string

The signing secret (`rng_init`) is obfuscated in the bundle via a timezone lookup table.

It can currently be retrieved by calling `window.rng.prototype.initialization()`.
It could be useful to retrieve the seeds from the js bundle and port the algorithm to rust.

```javascript
// Production: initialSeed("YWJiMjEzNjQ5NDVjMDU4MzMwOTY2N2", window.utimezone.berlin)
// berlin = index 53 in timezone array
// timezones[53] = { info: "QxM2NhM2Q5M2E=YmFhMDRlOTU2OD", extras: "E4NDM4ZGIzNTVlMjZlNjM1M2Y0ZTc=" }
//
// Algorithm: atob((seed + info + extras).substr(0, totalLength - 44))
// Result: "abb21364945c0583309667d13ca3d93a"
```

The bundle function is named `sha256` but actually computes MD5. This is intentional misdirection.

**Note**: Both `appSecret` and `rng_init` could change when Qobuz updates their bundle.
They are currently the same value but are derived differently and could diverge.

### Login (OAuth)

The old `POST /user/login` endpoint with email+password is **deprecated** (returns 401).
Authentication now requires an OAuth flow through the Qobuz web signin:

1. **Redirect user** to:
   ```
   https://www.qobuz.com/signin/oauth?ext_app_id={app_id}&redirect_url={callback_url}
   ```
   The user logs in on the Qobuz website (includes reCAPTCHA).

2. **Receive redirect** to `{callback_url}?code_autorisation={code}`

3. **Exchange code** for token:
   ```
   GET /oauth/callback?code={code}&private_key=6lz8C03UDIC7
   Headers: X-App-Id
   ```

   **Response**:
   ```json
   {
     "token": "qyinRoBk-...",
     "user_id": "1408119"
   }
   ```

4. Use `token` as the `X-User-Auth-Token` header for all subsequent requests.

The `private_key` (`6lz8C03UDIC7`) is a static value from the web player bundle.
The token is long-lived (weeks/months) and should be stored for reuse.

---

## Streaming

### Session Start

```
POST /session/start
Content-Type: application/x-www-form-urlencoded
Signed: yes
```

Must be called before requesting track URLs.

| Parameter | Value |
|-----------|-------|
| `profile` | `"qbz-1"` (fixed) |

Signature method name: `sessionstart`

**Response**:
```json
{
  "session_id": "abc123...",
  "expires_at": 1774500000,
  "infos": "hUnLXvp0zbBQ3bE3XplTLw.bm9uZQ"
}
```

- `session_id` — used as `X-Session-Id` header in subsequent requests
- `infos` — base64url-encoded values for key derivation (two parts separated by `.`)
- `expires_at` — session expiration Unix timestamp

### Get Track URL

```
GET /file/url
Signed: yes (with query string params included in signature)
Headers: X-Session-Id required
```

| Parameter | Description |
|-----------|-------------|
| `track_id` | Track ID (integer) |
| `format_id` | Audio format (see Format IDs) |
| `intent` | `"stream"`, `"download"`, or `"import"` |

Signature method name: `fileurl`

**Response**:
```json
{
  "url_template": "https://streaming.qobuz.com/.../segment/$SEGMENT$",
  "mime_type": "audio/mp4; codecs=\"flac\"",
  "n_segments": 26,
  "key_id": "...",
  "key": "qbz-1.{wrapped_key_b64}.{iv_b64}",
  "sampling_rate": 96000,
  "duration": 312.5,
  "n_samples": 30000000,
  "format_id": 7,
  "file_type": "full",
  "restrictions": [],
  "sample": false,
  "blob": "..."
}
```

- `url_template` — URL with `$SEGMENT$` placeholder (replace with 0, 1, 2, ..., n_segments-1)
- `n_segments` — total segments including init segment (segment 0)
- `key_id` — identifies the encryption key
- `key` — encrypted content key in format `qbz-1.{wrapped_key_base64}.{iv_base64}`
- `file_type` — `"full"` for complete track, `"preview"` for 30-second sample
- `blob` — opaque token used for streaming reports

### File Based Track URL streaming

```
GET /track/getFileUrl
Signed: yes
```

Same parameters and response as `/file/url`. This was the old endpoint before segmented
streaming. May still work but the web player now uses `/file/url`.

### Format IDs

| ID | Name | Quality |
|----|------|---------|
| 5 | MP3 | MP3 320kbps |
| 6 | CD | 16-bit / 44.1kHz FLAC |
| 7 | HIRES_96 | 24-bit / up to 96kHz FLAC |
| 27 | HIRES_192 | 24-bit / up to 192kHz FLAC |

---

## Segment Structure (CMAF/fMP4)

All segments are CMAF (Common Media Application Format) fragments, delivered as MP4 boxes.

### Segment 0 — Init Segment

Contains metadata needed to decode all subsequent segments:

- **`ftyp` box** — file type declaration
- **Custom UUID box** (`QBZ_INIT_UUID`: `c7c75df0fdd951e98fc22971e4acf8d2`):
  - FLAC STREAMINFO (sample rate, channels, bit depth)
  - Complete FLAC header (`fLaC` magic + STREAMINFO block)
  - `key_id` for encryption
  - **Segment table**: per-segment `{byte_len: u32, sample_count: u32}` — exact decrypted
    FLAC frame sizes, enabling byte-offset → segment-index mapping for seeking
- **`moov` box** — standard MP4 metadata (track, handler, sample description)

### Segments 1..N — Audio Segments

Each audio segment contains:

- **`styp` box** — segment type
- **Custom UUID box** (`QBZ_SEGMENT_UUID`: `3b42129256f35f75923663b69a1f52b2`):
  - Frame entry table: `[4B size][2B skip][2B flags][8B iv]` per frame
  - `flags = 0` → frame is unencrypted (cleartext FLAC)
  - `flags ≠ 0` → frame is encrypted (AES-128-CTR)
  - `iv` — 8 bytes used as nonce for CTR decryption
  - `skip` — bytes to skip at the start of the frame (typically 0)
- **`moof` box** — movie fragment header
- **`mdat` box** — actual audio data (FLAC frames, possibly encrypted)

### Segment Sizes

Segments are fixed in count and size per track — there is no API parameter to request
smaller segments. No HTTP range request support — each segment must be downloaded in full.

Typical sizes:

| Quality | Per-segment size | Segments for 4min track |
|---------|-----------------|------------------------|
| CD (16/44.1) | ~2-4 MB | ~10-15 |
| Hi-Res 96kHz | ~5-8 MB | ~15-25 |
| Hi-Res 192kHz | ~8-15 MB | ~20-30 |

---

## Encryption Scheme (qbz-1)

Three-step key derivation, then per-frame AES-128-CTR decryption.

### Step 1: Session Key (HKDF-SHA256)

```
rng_init  = hex_decode("abb21364945c0583309667d13ca3d93a")  // 16 bytes, from bundle.js
infos     = session_response.infos                           // e.g. "hUnLXvp0...Lw.bm9uZQ"
parts     = infos.split(".")
salt      = base64url_decode(parts[0])
info      = base64url_decode(parts[1])

session_key = HKDF-SHA256(ikm=rng_init, salt=salt, info=info, output_len=16)
```

### Step 2: Content Key Unwrap (AES-128-CBC)

```
key_str        = track_url.key              // "qbz-1.{wrapped}.{iv}"
key_parts      = key_str.split(".")         // 3 parts
wrapped_key    = base64url_decode(key_parts[1])
iv             = base64url_decode(key_parts[2])

content_key = AES-128-CBC-decrypt(key=session_key, iv=iv, data=wrapped_key)
              → PKCS7 unpad → 16 bytes
```

### Step 3: Per-Frame Decryption (AES-128-CTR)

For each encrypted frame (identified by `flags ≠ 0` in the QBZ_SEGMENT_UUID box):

```
nonce     = [8_byte_iv_from_frame_entry || 0x00 × 8]  // 16 bytes total
plaintext = AES-128-CTR(key=content_key, nonce=nonce, data=encrypted_frame_bytes)
```

Unencrypted frames (`flags = 0`) are passed through as-is.

### FLAC Reassembly

The decrypted output is a standard FLAC stream:
1. FLAC header from init segment's QBZ_INIT_UUID box (`fLaC` magic + STREAMINFO)
2. Concatenated decrypted frames from segments 1..N (in order)

This can be written directly to a `.flac` file.

---

## Content Endpoints

### Track

#### Get Track

```
GET /track/get
```

| Parameter | Description |
|-----------|-------------|
| `track_id` | Track ID |

**Response**: Full track metadata including album info, performer, duration, etc.

#### Get Multiple Tracks

```
POST /track/getList
Content-Type: application/json
```

**Body**:
```json
{
  "tracks_id": [123, 456, 789]
}
```

### Album

#### Get Album

```
GET /album/get
```

| Parameter | Description |
|-----------|-------------|
| `album_id` | Album ID |
| `offset` | Pagination offset (default 0) |
| `limit` | Results per page |
| `extra` | Extra fields to include (e.g. `"track_ids"`) |

#### Search Albums

```
GET /album/search
```

| Parameter | Description |
|-----------|-------------|
| `query` | Search query string |
| `offset` | Pagination offset |
| `limit` | Results per page |

#### Album Suggestions

```
GET /album/suggest
```

| Parameter | Description |
|-----------|-------------|
| `album_id` | Album ID to get suggestions for |

#### Album Story

```
GET /album/story
```

| Parameter | Description |
|-----------|-------------|
| `album_id` | Album ID |
| `offset` | Pagination offset |
| `limit` | Results per page |

Returns editorial content / liner notes for an album.

### Artist

#### Artist Page

```
GET /artist/page
```

| Parameter | Description |
|-----------|-------------|
| `artist_id` | Artist ID |

Returns the full artist page data (bio, discography, similar artists, etc.).

#### Get Releases List

```
GET /artist/getReleasesList
```

| Parameter | Description |
|-----------|-------------|
| `artist_id` | Artist ID |
| `release_type` | `"album"`, `"epSingle"`, `"live"`, `"compilation"` |
| `offset` | Pagination offset |
| `limit` | Results per page |

#### Similar Artists

```
GET /artist/getSimilarArtists
```

| Parameter | Description |
|-----------|-------------|
| `artist_id` | Artist ID |

#### Search Artists

```
GET /artist/search
```

| Parameter | Description |
|-----------|-------------|
| `query` | Search query string |
| `offset` | Pagination offset |
| `limit` | Results per page |

#### Artist Story

```
GET /artist/story
```

| Parameter | Description |
|-----------|-------------|
| `artist_id` | Artist ID |

### Playlist

#### Get Playlist

```
GET /playlist/get
```

| Parameter | Description |
|-----------|-------------|
| `playlist_id` | Playlist ID |
| `extra` | Extra fields (e.g. `"tracks"`) |
| `offset` | Track pagination offset |
| `limit` | Track results per page |

#### Get User Playlists

```
GET /playlist/getUserPlaylists
```

| Parameter | Description |
|-----------|-------------|
| `offset` | Pagination offset |
| `limit` | Results per page (max 500) |

#### Create Playlist

```
POST /playlist/create
Content-Type: application/x-www-form-urlencoded
```

| Parameter | Description |
|-----------|-------------|
| `name` | Playlist name |
| `description` | Playlist description |
| `is_public` | `true` / `false` |

#### Delete Playlist

```
POST /playlist/delete
Content-Type: application/x-www-form-urlencoded
```

| Parameter | Description |
|-----------|-------------|
| `playlist_id` | Playlist ID |

#### Add Tracks to Playlist

```
POST /playlist/addTracks
Content-Type: application/x-www-form-urlencoded
```

| Parameter | Description |
|-----------|-------------|
| `playlist_id` | Playlist ID |
| `track_ids` | Comma-separated track IDs |

#### Remove Tracks from Playlist

```
POST /playlist/deleteTracks
Content-Type: application/x-www-form-urlencoded
```

| Parameter | Description |
|-----------|-------------|
| `playlist_id` | Playlist ID |
| `playlist_track_ids` | Comma-separated playlist track IDs |

#### Reorder Tracks

```
POST /playlist/updateTracksPosition
Content-Type: application/x-www-form-urlencoded
```

| Parameter | Description |
|-----------|-------------|
| `playlist_id` | Playlist ID |
| `playlist_track_ids` | IDs to move |
| `insert_before` | Position to insert before |

#### Subscribe/Unsubscribe Playlist

```
POST /playlist/subscribe
POST /playlist/unsubscribe
```

| Parameter | Description |
|-----------|-------------|
| `playlist_id` | Playlist ID |

Adds/removes a public playlist from the user's library (favorites).

#### Search Playlists

```
GET /playlist/search
```

| Parameter | Description |
|-----------|-------------|
| `query` | Search query string |
| `offset` | Pagination offset |
| `limit` | Results per page |

#### Get Playlist Tags

```
GET /playlist/getTags
```

Returns the list of editorial playlist tags/categories.

#### Playlist Story

```
GET /playlist/story
```

| Parameter | Description |
|-----------|-------------|
| `playlist_id` | Playlist ID |

### Search

#### Global Search

```
GET /catalog/search
```

| Parameter | Description |
|-----------|-------------|
| `query` | Search query string |
| `type` | Content type filter (optional) |
| `offset` | Pagination offset |
| `limit` | Results per page |

Returns results across tracks, albums, artists, and playlists.

#### Dynamic Suggestions

```
POST /dynamic/suggest
Content-Type: application/json
```

Used for typeahead / autocomplete search suggestions.

### Favorites

#### Get Favorite IDs

```
GET /favorite/getUserFavoriteIds
```

| Parameter | Description |
|-----------|-------------|
| `limit` | Max results (web player uses 5000) |

Returns just the IDs of all favorited items (tracks, albums, artists). Lightweight
endpoint for checking favorite status.

#### Get Full Favorites

```
GET /favorite/getUserFavorites
```

| Parameter | Description |
|-----------|-------------|
| `type` | `"tracks"`, `"albums"`, or `"artists"` |
| `offset` | Pagination offset |
| `limit` | Results per page |

Returns full metadata for favorited items.

#### Add Favorites

```
POST /favorite/create
Content-Type: application/x-www-form-urlencoded
```

| Parameter | Description |
|-----------|-------------|
| `artist_ids` | Comma-separated artist IDs (optional) |
| `album_ids` | Comma-separated album IDs (optional) |
| `track_ids` | Comma-separated track IDs (optional) |

#### Remove Favorites

```
POST /favorite/delete
Content-Type: application/x-www-form-urlencoded
```

Same parameters as `/favorite/create`.

### Purchases

#### Get Purchase IDs

```
GET /purchase/getUserPurchasesIds
```

Returns IDs of all purchased items.

#### Get Purchases

```
GET /purchase/getUserPurchases
```

| Parameter | Description |
|-----------|-------------|
| `type` | Content type |
| `offset` | Pagination offset |
| `limit` | Results per page |

### Discovery / Editorial

#### Main Discovery

```
GET /discover/index
```

Returns the main discovery/home page data.

#### Featured Playlists

```
GET /discover/playlists
GET /playlist/getFeatured
```

| Parameter | Description |
|-----------|-------------|
| `type` | `"editor-picks"` etc. |
| `offset` | Pagination offset |
| `limit` | Results per page |

#### Featured Albums

```
GET /album/getFeatured
```

| Parameter | Description |
|-----------|-------------|
| `type` | See types below |
| `offset` | Pagination offset |
| `limit` | Results per page |

Featured album types:
- `"press-awards"` — Press award winners
- `"most-streamed"` — Most streamed albums
- `"new-releases-full"` — New releases
- `"qobuzissims"` — Qobuzissims selections
- `"ideal-discography"` — Ideal discography picks

#### Album of the Week

```
GET /discover/albumOfTheWeek
```

No parameters required.

### Genres

#### List Genres

```
GET /genre/list
```

Returns the complete genre hierarchy.

### User

#### Last Update

```
GET /user/lastUpdate
```

Returns timestamp of the last library change. Used to invalidate caches.

#### User Tracking

```
GET /user/tracking
Signed: yes
```

Returns user tracking/analytics data.

---

## Streaming Reports

These endpoints are used by the web player to report playback events for royalty
accounting and QoS monitoring. Third-party clients should implement these to ensure
artists receive proper streaming credits.

### Report Streaming Start

```
POST /track/reportStreamingStart
Content-Type: application/x-www-form-urlencoded
```

**Body**: `events` parameter containing JSON array:
```json
[{
  "track_id": 191642365,
  "date": 1774446600,
  "user_id": 12345,
  "format_id": 7
}]
```

### Report Streaming End

```
POST /track/reportStreamingEndJson
Content-Type: application/json
```

**Body**: Array of streaming report objects containing:
- Track ID, format ID, timestamps
- Duration listened, seek events
- `blob` token from the `file/url` response
- `renderer_context` — playback context info

### Report Streaming QoS

```
POST /event/reportStreamingQos
Content-Type: application/json
```

**Body**: `events` array:
```json
[{
  "type": "WAIT_BEFORE_PLAY",
  "track_id": 191642365,
  "format_id": 7,
  "date": 1774446600,
  "stream_source": "...",
  "duration": 3500,
  "number": 1,
  "network_type": "UNKNOWN",
  "bandwidth": "50Mo"
}]
```

QoS event types:
- `WAIT_BEFORE_PLAY` — initial buffering delay (> 2s)
- `WAIT_DURING_PLAY` — rebuffering during playback (> 3s)
- `UNRECOVERABLE` — playback failed entirely

### Report Player Issue

```
POST /event/reportPlayerIssue
Content-Type: application/json
```

Reports player errors (decoder failures, network issues, etc.).

### Report Track Context

```
POST /event/reportTrackContext
Content-Type: application/json
```

Reports the context in which a track was played (from search, playlist, album, etc.).

---

## QWS (Qobuz Web Socket)

The web player establishes a WebSocket connection for real-time events:

```
POST /qws/createToken
Content-Type: application/x-www-form-urlencoded
```

Returns a token used to authenticate the WebSocket connection. The WebSocket (`ws://`)
is used for features like Qobuz Connect (remote control between devices).

---

## Caching Behavior (Web Player)

The web player's built-in request cache:
- **Max entries**: 40 (LRU eviction)
- **TTL**: 1800 seconds (30 minutes)
- Requests with `limit=5000` bypass the cache
- Playlist caches can be selectively invalidated

---

## Network Request Flow (Page Load + Play)

Typical sequence when loading the web player and playing a track:

```
1.  GET  play.qobuz.com              → HTML shell
2.  GET  bundle.js                    → Application JavaScript (~7MB)
3.  OAuth flow (browser redirect)     → Auth token (via /oauth/callback)
4.  POST qws/createToken              → WebSocket token
5.  GET  user/tracking                → Analytics (signed)
6.  POST session/start                → Session ID + encryption infos
7.  WS   ws://...                     → WebSocket connection
8.  GET  file/url?track_id=...        → Segmented streaming URL (signed, X-Session-Id)
9.  GET  playlist/getUserPlaylists    → User library
10. GET  favorite/getUserFavoriteIds  → Favorite status
11. GET  user/lastUpdate              → Library sync check
12. GET  album/story                  → Editorial content
13. GET  segment/0                    → Init segment (FLAC header + crypto metadata)
14. GET  segment/1..N                 → Audio segments (encrypted FLAC frames)
15. POST track/reportStreamingStart   → Streaming report
16. POST track/reportStreamingEndJson → End-of-track report
```

---

## Web Player Implementation Notes

- Uses **Media Source Extensions (MSE)** with `SourceBuffer` in `"sequence"` mode
- Segments are appended to a `BufferQueue` as they download
- The `SEGMENT_TEMPLATE_PLACEHOLDER` is `"$SEGMENT$"` (replaced with segment index)
- Retry strategy array for failed segment downloads
- HLS file types: `PREVIEW = "preview"`, `FULL = "full"`

### Key Constants

```
QBZ_INIT_UUID              = "c7c75df0fdd951e98fc22971e4acf8d2"
QBZ_SEGMENT_UUID           = "3b42129256f35f75923663b69a1f52b2"
SEGMENT_TEMPLATE_PLACEHOLDER = "$SEGMENT$"
```

---

## Environment Configuration

The bundle contains configs for multiple environments:

| Environment | app_id | app_secret |
|-------------|--------|------------|
| integration | 377257687 | f686f063cb0841079d48495d4dea7cf2 |
| nightly | 377257687 | 05a4851e74ee47fda346f50cfdfc4f09 |
| recette | 724307056 | 05a4851e74ee47fda346f50cfdfc4f09 |
| **production** | **798273057** | **05a4851e74ee47fda346f50cfdfc4f09** |

Each environment has a different `rng_init` derived from different timezone seeds:

| Environment | Base64 Seed | Timezone | Index |
|-------------|------------|----------|-------|
| production | `YWJiMjEzNjQ5NDVjMDU4MzMwOTY2N2` | berlin | 53 |
| nightly/integration | `ODA2MzMxYzNiMGI2NDFkYTkyM2I4OT` | abidjan | 34 |
| recette | `ZjY5YTc3MzQ2ODZjYjk0Mjc2MjkzNz` | london | 37 |

The active environment is determined by `window.__ENVIRONMENT__`.
