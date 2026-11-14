# bbs757-webcdu

A web CDU for the **BlueBird Simulations 757** in Microsoft Flight Simulator. You get the full
CDU (live screen and every key) on a tablet, phone or any browser.

It's a single `bbs757-webcdu.exe`. The web page, fonts and `SimConnect.dll` are built in,
and there's nothing else to install.

## Using it

1. Close MobiFlight if it's running, because it uses the same port (8320).
2. Start `bbs757-webcdu.exe` on the sim PC, then load the 757.
   The window should show `SimConnect connected` and `aircraft connected: cdu-captain`.
3. On the tablet, phone or second PC, open `http://<sim-pc-ip>:8757`. On a tablet or phone,
   **Add to Home Screen** in the browser menu gets it full-screen. Windows Firewall needs to
   allow inbound TCP 8757.

The status dots at the top show the connections:

| dot    | green means                                                             |
|--------|-------------------------------------------------------------------------|
| server | the page is connected to the app                                        |
| sim    | the app is connected to MSFS through SimConnect (needed for key presses) |
| CDU    | the 757 is sending this seat's screen (CAPT and F/O are separate)       |

If **CDU** is green but **sim** is red, the screen updates but key presses go nowhere.

## How it works

- **Screen:** the 757 has built-in support for the WinWing/MobiFlight hardware CDU protocol.
  It connects as a websocket client to `ws://127.0.0.1:8320/winwing/cdu-captain` (and
  `cdu-co-pilot`), then pushes every screen update as JSON:
  `{"Target":"Display","Data":[ 14×24 cells of [char, colour, small, inverted] ]}`.
  The app takes MobiFlight's place on that port and passes the frames on to the web page.
- **Keys:** each CDU key is an L-var, `L:B757_CDU_<1|2>_<KEY>`, which is 1 while the key is
  held and 0 when it's released. The app writes these through SimConnect, one key at a time
  per CDU, with a minimum hold of 80 ms. The 757 only registers one held key per frame, so
  presses that overlap would be dropped.

## Building

See [`.github/workflows/build.yml`](.github/workflows/build.yml). Pushing a `v*` tag publishes
a GitHub release with the exe.

`SimConnect.dll` isn't committed to this repo. `scripts/fetch-simconnect.sh` downloads it
before the build. If a `SimConnect.dll` sits next to `bbs757-webcdu.exe`, that copy is used
instead of the built-in one.

## Credits

- [B612](https://github.com/polarsys/b612) fonts, SIL Open Font License 1.1 (`assets/fonts/OFL.txt`).
- The MobiFlight project's open-source CDU websocket hub, for showing the protocol.

## License

[PolyForm Noncommercial 1.0.0](LICENSE). You're free to use, modify and share bbs757-webcdu
for any noncommercial purpose. Forks are welcome. Streaming or recording while you use it is
explicitly fine, including monetized streams. Selling it, or putting it into a commercial
product or service, is not allowed without permission.
