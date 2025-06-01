# RatioUp

Tool to fake upload on your torrents. It can be useful on private or semi-private bittorrent trackers especially if you have a slow internet connection. Furthermore,
there are often many seeders so it becomes hard to seed and increase your ratio.

It is a tool like [JOAL](https://github.com/anthonyraymond/joal) or [Ratio Master](http://ratiomaster.net/).

I'm making this tool in order to practice Rust programming, having something lighter than Joal (written in Java) and that runs on any OS (I want to install it on my ARM NAS with only 2GB RAM).

## Disclamer

RatioUp is not designed to help or encourage you downloading illegal materials ! You must respect the law applicable in your country. I couldn't be held responsible for illegal activities performed by your usage of RatioUp.

I am not responsible if you get banned using this tool. However, you can reduce risk by using popular torrents (with many seeders and leechers).

## Changes (2025)

Because  I don't have much time to work on this project, I've decided to minimize features of this project.

It will simply load torrents from a given path before "seeding". I'll generate a static webpage/JSON to display stats
of the torrent files (the previous web UI was overkill).

## Installation

```shell
# Install Rust toolchain if not installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
# If installed, update it with
rustup update

# Go to the directory you want to install RatioUp (it will create the RatioUp folder)
git clone https://github.com/slundi/RatioUp.git && cd RatioUp

# Build the program for your arch
cargo build --release
ln -s target/release/RatioUp

# Edit the .env file according to your needs (nano .env)

# Run RatioUp everytime your machine reboot/startup
echo "@reboot cd $(pwd) && $(pwd)/RatioUp" | crontab -

# Uninstall rust if you do not need it
rustup self uninstall
```

## Configuration

Everything is contained in a `.env` file.

```ini
# Log level (available options are: INFO, WARN, ERROR, DEBUG, TRACE). Default is `INFO`.
LOG_LEVEL = INFO

# Client configuration
CLIENT = Transmission_3_00
# Torrent port, otherwise it is randomized
TORRENT_PORT = 56789

# Applicable speeds in bytes for each torrent
MIN_UPLOAD_RATE = 
MAX_UPLOAD_RATE =
MIN_DOWNLOAD_RATE = 
MAX_DOWNLOAD_RATE = 

# DIRECTORY WHERE TORRENTS ARE SAVED
TORRENT_DIR = "./torrents"

# Read only output file that contains the torrent list and few more information in order to be used for an external program
OUTPUT = "/var/www/ratioup.json"
```

Download and upload rates are in bytes (ie: 16MB = 16 x 1024 x 1024 = 16777216 bytes).
To disable downloads, set `min_download_rate` and `max_download_rate` to 0.

## Roadmap

- [x] Log using `tracing`
- [ ] use of XDG for the config file and logs
- [x] Change delay if different after announcing
- [x] Torrent clients in a separated library
- [x] Parse response instead of using REGEX
- [ ] Display session upload (global & per torrent)
- [x] Torrents with multiple trackers?
- [x] Drop torrent files from the web UI
- [x] BREAKING CHANGE: remove web UI
- [ ] Further testings (I use *rtorrent* and *qBittorrent*, other clients may not work properly)
- [ ] UDP announce URL support
- [x] Allow to generate a static JSON file with runtime statistics (global and per torrent download & upload, some torrent information), ie: `OUTPUT=/var/www/ratioup.json`
