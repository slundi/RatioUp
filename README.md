# RatioUp

Tool to fake upload on your torrents. It can be useful on private or semi-private bittorrent trackers especially if you have a slow internet connection.
It is a tool like [JOAL](https://github.com/anthonyraymond/joal) or [Ratio Master](http://ratiomaster.net/).
I'm making this tool in order to practice Rust programming.

## Disclamer

RatioUp is not designed to help or encourage you downloading illegal materials ! You must respect the law applicable in your country. I couldn't be held responsible for illegal activities performed by your usage of RatioUp.

## Deployment

```shell
docker run -d --name RatioUp --restart unless-stopped -v PATH:/data slundi/ratioup
```

Change the **PATH** in order to keep your configuration.
