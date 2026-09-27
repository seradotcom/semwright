# First-party driver portability audit

| integration | domain logic | Windows app | key assumption / status |
|---|---|---|---|
| Chromium / Playwright | high portability | yes | browser process/profile paths and native launch need Windows path review; good first proof |
| Blender | high portability | yes | localhost/app discovery and executable verification need Windows composition; domain protocol portable |
| LibreOffice | high portability | yes | UNO/process launch/path syntax need Windows fixture; domain semantics portable |
| KiCad | high portability | yes | CLI/process/path and packaged executable discovery need native verification |
| MLT video | high portability | binaries vary | melt/Kdenlive/Shotcut binary discovery and codecs are packaging concerns; timeline semantics portable |
| OBS | high portability | yes | websocket protocol portable; AppContainer loopback/network grants must never be globally relaxed |

Live Windows portability for each application integration requires separate native evidence. Recommended proofs after platform CI include the Chromium noninteractive profile fixture and OBS driver verification with network policy denied unless explicitly granted.
