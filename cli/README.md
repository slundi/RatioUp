# RatioUp CLI

RatioUp without the web server (and UI). It runs as a daemon to handle events properly.

## Roadmap

- [ ] Parse CLI:
  - [ ] torrent folder
  - [ ] min/max upload rate, min/max download rate
  - [ ] client
  - [ ] output JSON file
  - [ ] history in order to show lasts speeds for each torrent (define the time window or number of saved values per torrent)
- [ ] Check rates (min <= max)
- [ ] Check torrent folder and create it if not exists
- [ ] Multithread:
  - [ ] Load torrents at startup and every time we refresh
  - [ ] Announce
  - [ ] Daemon stop (interrupts)
- [ ] Generate JSON output if applicable
