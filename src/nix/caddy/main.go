package main

import (
	caddycmd "github.com/caddyserver/caddy/v2/cmd"

	_ "github.com/caddyserver/caddy/v2/modules/standard"
	_ "github.com/silinternational/certmagic-storage-dynamodb/v3"
)

func main() {
	caddycmd.Main()
}
