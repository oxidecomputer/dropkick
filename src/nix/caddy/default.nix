# The reason we maintain our own recipe of caddy is to have the DynamoDB
# plugin, something upstream nixpkgs caddy doesn't have.
{ buildGoModule, fetchFromGitHub }:
let
  # To update caddy:
  # - Go to https://github.com/NixOS/nixpkgs/blob/master/pkgs/servers/caddy/default.nix to see what the latest data is. From this file, copy
  #   - version = 
  #   - hash = from inside the `dist = fetchFromGithub` line
  #
  # Then you should `cd` into the `caddy` dir (the one containing the nix file
  # you're reading right now), and run:
  # 
  # go get github.com/caddyserver/caddy/v2
  # go get github.com/silinternational/certmagic-storage-dynamodb/v3
  # go mod tidy
  # rm -r vendor || true
  # go mod vendor
  #
  # Now run the following command, and replace the `vendorHash` value with the
  # output:
  # nix-hash --sri --type sha256 vendor
  #
  # After all this, re-run dropkick to get the new `vendorHash`
  version = "2.7.6";
  dist = fetchFromGitHub {
    owner = "caddyserver";
    repo = "dist";
    rev = "v${version}";
    hash = "sha256-aZ7hdAZJH1PvrX9GQLzLquzzZG3LZSKOvt7sWQhTiR8=";
  };
in
buildGoModule {
  pname = "caddy";
  inherit version;
  src = ./.;
  vendorHash = "sha256-3tAnyz+v/4BCUcnYUvw/vUNDahPm6pKuhQvvGRw/2jY=";

  postInstall = ''
    install -Dm644 ${dist}/init/caddy.service -t $out/lib/systemd/system
    substituteInPlace $out/lib/systemd/system/caddy.service --replace "/usr/bin/caddy" "$out/bin/caddy"
  '';
}
