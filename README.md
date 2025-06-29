# RatioUp

Tool to fake upload on your torrents. It can be useful on private or semi-private bittorrent trackers especially if you have a slow internet connection. Furthermore,
there are often many seeders so it becomes hard to seed and increase your ratio.

It is a tool like [JOAL](https://github.com/anthonyraymond/joal) or [Ratio Master](http://ratiomaster.net/).

I'm making this tool in order to practice Rust programming, having something lighter than Joal (written in Java) and that runs on any OS (I want to install it on my ARM NAS with only 2GB RAM).

## Disclamer

RatioUp is not designed to help or encourage you downloading illegal materials ! You must respect the law applicable in your country. I couldn't be held responsible for illegal activities performed by your usage of RatioUp.

I am not responsible if you get banned using this tool. However, you can reduce risk by using popular torrents (with many seeders and leechers).

## Breaking changes (2025)

Because  I don't have much time to work on this project, I've decided to minimize features of this project.

It will simply load torrents from a given path before "seeding". I'll generate a static webpage/JSON to display stats
of the torrent files (the previous web UI was overkill).

## Repositories

* [Codeberg](https://codeberg.org/slundi/RatioUp/): up to date sources
* [Github](https://github.com/slundi/RatioUp) (mirror): for community tickets, publishing releases, repo backup

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

# Run RatioUp everytime your machine reboot/startup
echo "@reboot cd $(pwd) && $(pwd)/RatioUp" | crontab -

# Uninstall rust if you do not need it
rustup self uninstall
```

## Configuration

On the `XDG_CONFIG_DIR`, usually `~/.config/RatioUp/`, you can create a `config.toml` file with:

```toml
# Choose a client
# List is available there: https://docs.rs/fake-torrent-client/0.9.6/fake_torrent_client/clients/enum.ClientVersion.html
client = "Transmission_3_00"
port = 55555
numwant = 8

# path will be $XDG_RUNTIME_DIR/ratio_up.pid
use_pid_file = true

# configure range of speed in bytes for each torrent
min_upload_rate = 262144
max_upload_rate = 23068672

# Will load torrent from `XDG_CONFIG_DIR` by default but you can customize it.
torrent_dir = "./torrents"

# If given, it will output stats in a JSON file that you can use in a webserver to track what is happening.
output_stats = "/tmp/RatioUp.json"
```

Upload rates are in bytes (ie: 16MB = 16 x 1024 x 1024 = 16777216 bytes). It is only seeding, it does not fake the downloading first.

## Roadmap

- [x] Change delay if different after announcing
- [ ] Display session upload (global & per torrent)
- [x] Torrents with multiple trackers?
- [ ] Further testings (I use *rtorrent* and *qBittorrent*, other clients may not work properly)
- [ ] UDP announce URL support
- [x] Allow to generate a static JSON file with runtime statistics (global and per torrent download & upload, some torrent information), ie: `OUTPUT=/var/www/ratioup.json`
- [ ] Generate a fancy web page, if nobody is helping me for it, it will never be done
