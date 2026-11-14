# webcdu

A web CDU for the **BlueBird Simulations 757** in Microsoft Flight Simulator. You get the full
CDU (live screen and every key) on an iPad, tablet or any browser, and you can type into it
with a keyboard.

It's a single `webcdu.exe`. The web page, fonts and `SimConnect.dll` are built in, and there's
nothing else to install.

## Using it

1. Close MobiFlight if it's running, because it uses the same port (8320).
2. Start `webcdu.exe` on the sim PC, then load the 757.
   The window should show `SimConnect connected` and `aircraft connected: cdu-captain`.
3. On the tablet, open `http://<sim-pc-ip>:8757`. On an iPad, use Share → **Add to Home Screen**
   to get it full-screen. Windows Firewall needs to allow inbound TCP 8757.

The status dots at the top show the connections:

| dot    | green means                                                                  |
|--------|------------------------------------------------------------------------------|
| server | the page is connected to webcdu                                              |
| sim    | webcdu is connected to MSFS through SimConnect (needed for key presses)      |
| CDU    | the 757 is sending this seat's screen (CAPT and F/O are separate)            |

If **CDU** is green but **sim** is red, the screen updates but key presses go nowhere.

### Keyboard

| key                        | CDU key            |
|----------------------------|--------------------|
| A–Z, 0–9, `.` `/` space    | same               |
| `-`                        | +/− (once)         |
| `+`                        | +/− (twice)        |
| Backspace / Delete         | CLR / DEL          |
| Enter                      | EXEC               |
| F1–F6 / F7–F12             | left / right LSK 1–6 |
| PgUp / PgDn / Home         | PREV / NEXT / MENU |

## How it works

- **Screen:** the 757 has built-in support for the WinWing/MobiFlight hardware CDU protocol.
  It connects as a websocket client to `ws://127.0.0.1:8320/winwing/cdu-captain` (and
  `cdu-co-pilot`), then pushes every screen update as JSON:
  `{"Target":"Display","Data":[ 14×24 cells of [char, colour, small, inverted] ]}`.
  webcdu takes MobiFlight's place on that port and passes the frames on to the web page.
- **Keys:** each CDU key is an L-var, `L:B757_CDU_<1|2>_<KEY>`, which is 1 while the key is
  held and 0 when it's released. webcdu writes these through SimConnect, one key at a time
  per CDU, with a minimum hold of 80 ms. The 757 only registers one held key per frame, so
  presses that overlap would be dropped.

## Building

Builds are cross-compiled from Linux with MinGW-w64, and CI does the same
(`.github/workflows/build.yml`). Pushing a `v*` tag publishes a GitHub release with the exe.

```sh
rustup target add x86_64-pc-windows-gnu
# plus the MinGW-w64 gcc package for your distro (Debian/Ubuntu: gcc-mingw-w64-x86-64)
scripts/fetch-simconnect.sh            # SimConnect.dll -> vendor/ (pinned SHA-256)
cargo build --release --target x86_64-pc-windows-gnu
```

`SimConnect.dll` comes from the MSFS SDK (via the `SimConnect` package on PyPI) and is not
committed to this repo. If a `SimConnect.dll` sits next to `webcdu.exe`, that copy is used
instead of the built-in one.

## Credits

- [B612](https://github.com/polarsys/b612) fonts, SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).
- The MobiFlight project's open-source CDU websocket hub, for showing the protocol.
