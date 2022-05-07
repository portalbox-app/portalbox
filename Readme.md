## Introduction
PortalBox is a collection of tools to make your dev machine web-accessible.

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

The dashboard will be available at http://localhost:3030 by default.



## SSH Jump Host
Once signed in, SSH would be available using the built-in jump host. To SSH into your dev machine from another machine:

-  With the `portalbox` client
```
ssh -o ProxyCommand="portalbox tunnel {BASE_SUB_DOMAIN}" {USER}@{DEVBOX}
```
- With `openssh s_client`
```
ssh -o ProxyCommand="openssl s_client -quiet -connect {BASE_SUB_DOMAIN}-ssh.portalbox.app:22857 -servername {BASE_SUB_DOMAIN}-ssh.portalbox.app" {USER}@{DEVBOX}
```

The above commands are compatible with VSCode remote development.

## The PortalBox Client


```
portalbox 
The PortalBox Client

USAGE:
    portalbox [OPTIONS] [SUBCOMMAND]

OPTIONS:
        --config-file <CONFIG_FILE>    Custom config file location
    -h, --help                         Print help information

SUBCOMMANDS:
    config     Show current config
    help       Print this message or the help of the given subcommand(s)
    reset      Reset data
    start      Start the portalbox client
    tunnel     Create a tunnel usable by ssh ProxyCommand
    version    Show current version
```
