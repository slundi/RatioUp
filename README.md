# RatioUp

Tool to fake upload on your torrents.

## Features

* WebUI
* Can manage multiple torrents

## Deployment

```shell
docker run -d --name RatioUp --restart unless-stopped -v PATH:/data slundi/ratioup
```

Change the **PATH** in order to keep your configuration.
