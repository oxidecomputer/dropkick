{ buildGoModule, fetchFromGitHub }:
let
  version = "2.6.4";
  dist = fetchFromGitHub {
    owner = "caddyserver";
    repo = "dist";
    rev = "v${version}";
    hash = "sha256-SJO1q4g9uyyky9ZYSiqXJgNIvyxT5RjrpYd20YDx8ec=";
  };
in
buildGoModule {
  pname = "caddy";
  inherit version;
  src = ./.;
  vendorHash = "sha256-mdLhXLCzgAfO0Dv3aOlEiMhl6u/a/smL3CVki0aB1k0=";

  postInstall = ''
    install -Dm644 ${dist}/init/caddy.service -t $out/lib/systemd/system
    substituteInPlace $out/lib/systemd/system/caddy.service --replace "/usr/bin/caddy" "$out/bin/caddy"
  '';
}
