# TimeAtlas

World clocks, weather and a map in one native macOS window, written in Rust
with [iced](https://iced.rs). No web view, no account, no API keys.

- **Clocks:** digital and analogue time for 2 to 10 places, sorted west to east,
  with each place's GMT offset.
- **Weather:** current conditions, today's high and low, and a forecast for the
  next 24 hours and 7 days (click a card). The last good data is kept, so the
  app opens with weather even when offline.
- **Map:** a native, draggable and zoomable map with live time labels, tile
  caching, automatic retries, a reload button and a "show all places" button.
  Choose the map style in Settings.
- **Switches:** turn the map and weather on or off from the top bar; set what
  shows at launch in Settings.
- **Resizable:** text and layout scale with the window, and narrow windows
  switch to a stacked layout. Light and dark mode follow macOS.

## Running it

Requires Rust (stable, 1.88 or later) and Xcode Command Line Tools.

```sh
cargo run --release
```

## Building the Mac app

```sh
macos/build.sh                 # TimeAtlas.app for this Mac, in dist/
macos/build.sh --developer-id  # signed, notarized .app and .dmg
macos/build.sh --app-store     # signed .pkg for App Store Connect
```

Run `macos/build.sh --help` for the options. In short:

- **Developer ID** needs a "Developer ID Application" certificate in your
  keychain and, once, a notarization profile:
  `xcrun notarytool store-credentials TimeAtlas --apple-id <you> --team-id <TEAMID>`.
- **Mac App Store** needs an "Apple Distribution" certificate, a Mac installer
  certificate, an App ID matching `BUNDLE_ID`, and its Mac App Store
  provisioning profile (`PROVISIONING_PROFILE=/path/to/profile`). Upload the
  resulting `.pkg` with Apple's Transporter app.

The default bundle ID is `io.github.sicambium.timeatlas`; set `BUNDLE_ID` to
use your own.

### App Store readiness

- Runs in the **App Sandbox** with a single entitlement: outgoing network
  connections (`macos/TimeAtlas.entitlements`).
- Ships a **privacy manifest** (`macos/PrivacyInfo.xcprivacy`): no tracking and
  no data collected. The only "required reason" API is reading the dates of its
  own cached map tiles.
- Built with the **hardened runtime**, a full icon set (`macos/AppIcon.icns`,
  regenerate with `python3 tools/make_icon.py`), minimum macOS 12, and the
  export-compliance flag set (standard HTTPS only).
- Data sources are credited in the app: Open-Meteo for weather and the chosen
  map provider's attribution on the map.

Before a public release, check the map provider's terms: the OpenStreetMap
and OpenTopoMap tile servers are run by volunteers and discourage heavy use
by widely distributed apps; CARTO's free basemaps are for non-commercial use.
Providers are defined in `src/tiles.rs`.

## Where things live

| Path | What |
|---|---|
| `src/main.rs` | App state, update logic and layout |
| `src/api.rs` | Open-Meteo city search and weather |
| `src/map.rs`, `src/tiles.rs` | Map drawing, tile download queue and cache |
| `src/graphics.rs` | Weather icons, clocks and UI glyphs (vector) |
| `src/style.rs` | Colours, fonts and widget styles |
| `src/store.rs` | Saved places, preferences and weather cache |
| `macos/` | Info.plist, entitlements, privacy manifest, icon, build script |

Settings are stored in `~/Library/Application Support/TimeAtlas/` (inside the
app's sandbox container when sandboxed), and map tiles are cached in
`~/Library/Caches/TimeAtlas/`.

## Credits

Weather and geocoding: [Open-Meteo](https://open-meteo.com) (CC BY 4.0).
Map data © [OpenStreetMap](https://www.openstreetmap.org/copyright) contributors;
tiles by OpenStreetMap, [CARTO](https://carto.com/attributions) or
[OpenTopoMap](https://opentopomap.org). Font: [Inter](https://rsms.me/inter/)
(SIL Open Font License).

The original Go/Wails version is still in the repository (`app.go`,
`frontend/`) for reference and will be removed once the Rust app is confirmed.
