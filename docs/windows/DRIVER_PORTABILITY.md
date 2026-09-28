# First-party driver portability audit

| integration | domain logic | Windows app | key assumption / status |
|---|---|---|---|
| Chromium / Playwright | high portability | yes | browser process/profile paths and native launch need real Windows app evidence; domain protocol portable |
| Blender | high portability | yes | localhost/app discovery and executable verification compose with the Windows secure-spawn boundary; real Windows Blender evidence still required |
| LibreOffice | high portability | yes | UNO/process launch/path syntax need a Windows fixture; domain semantics portable |
| KiCad | high portability | yes | CLI/process/path and packaged executable discovery need native Windows verification |
| MLT video | high portability | binaries vary | melt/Kdenlive/Shotcut discovery and codecs are packaging concerns; timeline semantics portable |
| OBS | high portability | yes | websocket protocol portable; Host-mediated loopback and owner-gated ambient network are separate Windows authorities and must remain explicit |

The Windows platform can now securely spawn Driver, Plugin and governed stdio MCP children and provide the proven Driver/Plugin authority profiles. This does not certify individual Windows application integrations. Each integration still needs real app/version/path/process evidence before it can be labeled live on Windows.
