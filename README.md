# RatioUp

Tool to fake upload on your torrents. It can be useful on private or semi-private bittorrent trackers especially if you have a slow internet connection. Furthermore,
there are often many seeders so it becomes hard to seed and increase your ratio.

It is a tool like [JOAL](https://github.com/anthonyraymond/joal) or [Ratio Master](http://ratiomaster.net/).

I'm making this tool in order to practice Rust programming, having something lighter than Joal (written in Java) and that runs on any OS (I want to install it on my ARM NAS with only 2GB RAM).

## Disclamer

RatioUp is not designed to help or encourage you downloading illegal materials ! You must respect the law applicable in your country. I couldn't be held responsible for illegal activities performed by your usage of RatioUp.

I am not responsible if you get banned using this tool. However, you can reduce risk by using popular torrents (with many seeders and leechers).

## Installation

### Command line interface (CLI)

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

### With Docker

Using a terminal:

```shell
docker run --name RatioUp -p 0.0.0.0:8070:8070 slundi/ratioup:<tag>
```

*For now I did not manage to build a multi-arch docker image so you need to specify the [image tag](https://hub.docker.com/r/slundi/ratioup/tags)*

You can change `-p 0.0.0.0:8070:8070` to manage your access through your prefered port.

You can add `-e WEBROOT=/my-path/` if you want to change your root URL. By default, it is `/`.

### Health check

**When the web UI is enabled**, you can check the health of the service.

With **Docker**, you need to edit the [Dockerfile](Dockerfile) by adding this line: `HEALTHCHECK CMD curl --fail http://<ip>:<port>/health || exit 1`

If you use **docker-compose**, do something like that (change times to your convenience):

```yaml
version: '3.4'
services:
  ratioup:
    image: slundi/ratioup
    restart: unless-stopped
    ports:
      - "8070:8070"
    healthcheck:
      test: curl --fail http://localhost:8070/health || exit 1  # with wget: wget --no-verbose --tries=1 --spider http://localhost:8070/health || exit 1
      interval: 60s
      retries: 5
      start_period: 20s
      timeout: 10s
```

## Configuration

Everything is contained in a `.env` file.

```ini
# Log level (available options are: INFO, WARN, ERROR, DEBUG, TRACE). Default is `INFO`.
LOG_LEVEL = INFO

# Web serveur configuration
# HTTP web port
HTTP_PORT = 8070
#Custom web root
#WEB_ROOT = "/ratioup/"

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
```

Download and upload rates are in bytes (ie: 16MB = 16 x 1024 x 1024 = 16777216 bytes).
To disable downloads, set `min_download_rate` and `max_download_rate` to 0.

## Security

For now, I'm not planning add a security layer because I'll use it on my home lan network. If you want to secure it, you can use a reverse proxy with **nginx** (and any other web server you ar familiar with) and add a SSL layer and a basic authentication. You can also contribute by adding a basic auth.

### Nginx reverse proxy

1. Edit `/etc/nginx/sites-available/ratioup` and set your configuration:

```nginx
  location / {  #if you change "/" with another path, you must set the web root on the CLI
    #if you want a basic auth, remove the # of the following 2 lines
    #auth_basic “Restricted Area”;
    #auth_basic_user_file /path/to/the/password/file/.my_password_file;

    proxy_pass http://127.0.0.1:8070;
  }
```

2. Enable the new site: `sudo ln -s /etc/nginx/sites-available/ratioup /etc/nginx/sites-enabled/ratioup`
3. Check nginx configuration: `sudo nginx -t`
4. Reaload nginx with the new configuration: `sudo nginx -s reload` or `sudo systemctl reload nginx` or `sudo service nginx reload` (Debian/Ubuntu) or `sudo /etc/init.d/nginx reload` (CentOS,Fedora/...)

### Basic auth

1. `sudo apt install apache2-utils` or `sudo apt install httpd-tools`
2. Create a user with and **new file** with `sudo htpasswd -c /path/to/the/password/file/.my_password_file user1`, if the file already exists you just need to remove the `-c`: `sudo htpasswd /path/to/the/password/file/.my_password_file user1`
3. Check nginx configuration: `sudo nginx -t`
4. Reaload nginx with the new configuration: `sudo nginx -s reload` or `sudo systemctl reload nginx` or `sudo service nginx reload` (Debian/Ubuntu) or `sudo /etc/init.d/nginx reload` (CentOS,Fedora/...)

## Roadmap

- [x] Change delay if different after announcing
- [x] Torrent clients in a separated library
- [x] Parse response instead of using REGEX
- [ ] Display session upload (global & per torrent)
- [x] Torrents with multiple trackers?
- [x] Drop torrent files from the web UI
- [ ] Retracker torrents
- [ ] Further testings (I use *rtorrent* and *qBittorrent*, other clients may not work properly)
- [ ] UDP announce URL support
- [ ] Improve health check by probing the announcer and the key refresh when applicable
