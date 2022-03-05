# RatioUp

Tool to fake upload on your torrents. It can be useful on private or semi-private bittorrent trackers especially if you have a slow internet connection. Furthermore, 
there are often many seeders so it becomes hard to seed and increase a ratio.
It is a tool like [JOAL](https://github.com/anthonyraymond/joal) or [Ratio Master](http://ratiomaster.net/).
I'm making this tool in order to practice Rust programming, having something lighter than Joal (written in Java) and that runs on any OS.

## Disclamer

RatioUp is not designed to help or encourage you downloading illegal materials ! You must respect the law applicable in your country. I couldn't be held responsible for illegal activities performed by your usage of RatioUp.

## Deployment

```shell
docker run -d --name RatioUp --restart unless-stopped -v PATH:/data slundi/ratioup
```

Change the **PATH** in order to keep your configuration.
