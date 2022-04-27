## Introduction
PortalBox is a suite of tools to make your dev machine web-accessible.

Features:
- Self-hosted Visual Studio Code server
- Web terminal
- Reverse proxy to make everything accessible online

## Installation
On Linux/Mac
```
brew tap portalbox-app/tap
brew install portalbox
```

On Windows
```
scoop bucket add portalbox https://github.com/portalbox-app/scoop-bucket
scoop install portalbox
```

## Run
```
portalbox start
```

## The PortalBox Client


```
portalbox 
The PortalBox Client

USAGE:
    portalbox [OPTIONS] <SUBCOMMAND>

OPTIONS:
        --config-file <CONFIG_FILE>    Custom config file location
    -h, --help                         Print help information

SUBCOMMANDS:
    config    Show current config
    help      Print this message or the help of the given subcommand(s)
    reset     Reset data
    start     Start the portalbox client
```
