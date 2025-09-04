# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    crane.url = "github:ipetkov/crane/v0.21.0";
    nixie-tubes.url = "github:oxidecomputer/nixie-tubes";
  };

  outputs = { nixpkgs, rust-overlay, crane, nixie-tubes, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { };

      dropkickInput = pkgs.lib.importJSON ./input.json;
      dynamodbStorage = dropkickInput.certStorage == "Dynamodb";
      nixpkgsInput = map (s: builtins.getAttr s pkgs) dropkickInput.nixpkgs;

      interactiveShell = dropkickInput.allowSsh || dropkickInput.allowAwsSsm;
    in
    rec {

      packages."${system}" = {
        default =
          let
            pkgs = import nixpkgs {
              inherit system;
              overlays = [
                (import rust-overlay)
              ];
            };
            toolchain =
              if (dropkickInput.toolchainFile == null)
              then pkgs.rust-bin.stable.latest.minimal
              else (pkgs.rust-bin.fromRustupToolchainFile (/. + dropkickInput.toolchainFile));
            crane' = (crane.mkLib pkgs).overrideToolchain toolchain;
          in
          crane'.buildPackage {
            src = pkgs.nix-gitignore.gitignoreSource [ ] (/. + dropkickInput.workspaceRoot);

            pname = dropkickInput.package.name;
            version = dropkickInput.package.version;

            # Only build the binary we want.
            cargoExtraArgs = "--package ${dropkickInput.package.name}";
            doCheck = false;

            nativeBuildInputs = nixpkgsInput;
            buildInputs = nixpkgsInput;
          };
      };

      nixosConfigurations.dropkick = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          nixie-tubes.nixosModules.ssh-init

          ({ config, lib, pkgs, modulesPath, ... }: {
            imports = [
              (modulesPath + "/installer/cd-dvd/iso-image.nix")
            ];

            system.stateVersion = lib.trivial.release;

            # If our service needs network-online.target, it almost
            # certainly needs IPv4. (In EC2, IPv6 seems to come up first;
            # this is also necessary to get anything from IMDS.)
            networking.dhcpcd.wait = "ipv4";

            systemd.services.dropshot-server = {
              wantedBy = [ "multi-user.target" ];
              after = [ "network-online.target" ];
              wants = [ "network-online.target" ];
              before = [ "caddy.service" ];
              serviceConfig = {
                EnvironmentFile = lib.mkIf (dropkickInput.envFile != null) (pkgs.copyPathToStore (/. + dropkickInput.envFile));
                ExecStart = "${packages."${system}".default}/bin/${dropkickInput.binName} ${dropkickInput.runArgs}";
                Restart = "on-failure";

                # sandboxing and other general security:
                # (see systemd.exec(5) and `systemd-analyze security dropshot-server.service`)
                ProtectProc = "invisible";
                DynamicUser = true;
                CapabilityBoundingSet = "";
                UMask = "0077";
                ProtectHome = true;
                PrivateDevices = true;
                PrivateUsers = true;
                ProtectHostname = true;
                ProtectClock = true;
                ProtectKernelTunables = true;
                ProtectKernelModules = true;
                ProtectKernelLogs = true;
                ProtectControlGroups = true;
                RestrictAddressFamilies = "AF_INET AF_INET6 AF_UNIX";
                RestrictNamespaces = true;
                LockPersonality = true;
                MemoryDenyWriteExecute = true;
                RestrictRealtime = true;
                SystemCallFilter = "@system-service";
                SystemCallErrorNumber = "EPERM";
                SystemCallArchitectures = "native";
              };
            };

            services.caddy = {
              enable = true;
              package = pkgs.caddy.withPlugins {
                plugins = ["github.com/silinternational/certmagic-storage-dynamodb/v3@v3.1.1"];
                # If you get a hash mismatch, or to update the plugins, replace this with an empty
                # string, and do a build: nix will show you the correct hash.
                hash = "sha256-aQ20My8nK1n66kWEeRWWzmwjXJSiIL7ytAOOHXalmD8=";
              };

              email = "lets-encrypt@oxidecomputer.com";
              acmeCA = lib.mkIf dropkickInput.testCert "https://acme-staging-v02.api.letsencrypt.org/directory";

              # Set up reverse proxy.
              # tls.on_demand is used because Caddy will start up and request a certificate before it is accessible.
              virtualHosts."${dropkickInput.hostname}".extraConfig = ''
                tls {
                  on_demand
                }
                reverse_proxy :${toString dropkickInput.port}
              '';

              # Configure on_demand_tls, per https://caddyserver.com/docs/automatic-https#on-demand-tls.
              # This shouldn't be necessary because we aren't using any wildcards with tls.on_demand enabled.
              # But hey, a self-contained implementation is trivial and better safe than sorry.
              globalConfig = ''
                on_demand_tls {
                  ask http://localhost:478/check
                }
                # disable the zerossl issuer
                cert_issuer acme
              '' + lib.strings.optionalString dynamodbStorage ''
                # store certificates in DynamoDB
                storage dynamodb {$DROPKICK_CERTIFICATE_TABLE}
              '';
              # Set up on_demand_tls.ask responder.
              virtualHosts."http://localhost:478".extraConfig = ''
                @valid {
                  path /check
                  query domain=${dropkickInput.hostname}
                }
                respond @valid 200
                respond 404
              '';
            };

            # Create a service to populate /run/dropkick-caddy.env.
            systemd.services.dropkick-caddy-env = lib.mkIf dynamodbStorage {
              wantedBy = [ "multi-user.target" ];
              after = [ "network-online.target" ];
              wants = [ "network-online.target" ];
              before = [ "caddy.service" ];

              script = ''
                set -euo pipefail

                curl_retry() {
                  ${pkgs.curl}/bin/curl --silent --show-error \
                    --retry 10 --retry-delay 1 --fail --connect-timeout 1 "$@"
                }
                token=$(curl_retry -X PUT -H "X-aws-ec2-metadata-token-ttl-seconds: 60" \
                  http://169.254.169.254/latest/api/token)
                metadata() {
                  curl_retry -H "X-aws-ec2-metadata-token: $token" \
                    "http://169.254.169.254/latest/meta-data/$1" | head -n 1
                }
                echo "AWS_REGION=$(metadata placement/region)" >/run/dropkick-caddy.env
                echo "DROPKICK_CERTIFICATE_TABLE=$(metadata tags/instance/dropkick:certificate-table)" >>/run/dropkick-caddy.env
              '';

              serviceConfig = {
                Type = "oneshot";
                RemainAfterExit = true;
              };
            };
            systemd.services.caddy.serviceConfig.EnvironmentFile = lib.mkIf dynamodbStorage "/run/dropkick-caddy.env";

            # The firewall is enabled by default. Enabling SSH automatically allows port 22 through the
            # firewall, but enabling Caddy does not allow any ports.
            networking.firewall = {
              enable = true;
              allowedTCPPorts = [ 80 443 ];
            };

            # In EC2 we need to use two network interfaces; by default, dhcpcd sets up a default route on
            # both interfaces, but the first interface takes priority. If outbound traffic from the second
            # interface's source address goes out the first interface, EC2 drops it (and even if they
            # didn't, it would cause other problems). This dhcpcd hook sets up route table rules to ensure
            # outbound traffic with the second interface's source address goes out the second interface.
            networking.dhcpcd.runHook = ''
              dropkick_hook() {
                local ip=${pkgs.iproute2}/bin/ip
                local jq=${pkgs.jq}/bin/jq
                local address gateway ifindex proto

                # The first interface is already the default route in the default route table, so skip it here.
                if [[ $interface_order == $interface* ]]; then
                  return
                fi

                # Use the ifindex as the table ID.
                ifindex=$(cat /sys/class/net/"$interface"/ifindex)

                for proto in "-4" "-6"; do
                  gateway=$($ip -json $proto route show default dev "$interface" | $jq -r '.[0].gateway')
                  if [[ -n $gateway ]]; then
                    $ip $proto route replace default via "$gateway" dev "$interface" table "$ifindex"

                    for address in $($ip -json $proto address show "$interface" scope global | $jq -r '.[].addr_info[].local | select(.)'); do
                      if [[ -z $($ip $proto rule list from "$address") ]]; then
                        $ip $proto rule add from "$address" table "$ifindex"
                      fi
                    done
                  else
                    $ip $proto route del default table "$ifindex"
                  fi
                done
              }
              dropkick_hook
            '';

            isoImage.appendToMenuLabel = "";
            isoImage.makeEfiBootable = true;
            isoImage.squashfsCompression = "zstd -Xcompression-level 3";

            # Persistent storage setup.
            #
            # We'd like to be able to persist some things through unexpected host reboots, e.g.
            # certificates fetched from Let's Encrypt, host SSH keys, logs. Our boot image is a read-only
            # ISO9660 volume. There is a pattern among NixOS users to symlink locations like /var/log into
            # a /persist volume: https://grahamc.com/blog/erase-your-darlings
            #
            # Disks in EC2 and Oxide are measured in whole gibibytes, but our bootable ISO is less than
            # half that. We use the remaining space on the disk for the /persist volume -- this is simpler
            # than needing to figure out where exactly another blank disk that is attached to the system
            # is across the varied hardware of EC2 and what the Oxide rack exposes.
            #
            # Setup is pretty simple: during the stage 1 script but before mounting /dev/root, use dmsetup
            # to split the disk into a "bootiso" device and "persist" device.
            boot.initrd.postDeviceCommands = ''
              dropkick_dmsetup() {
                local isodisk isodisksz isopartend
                isodisk=$(blkid -t TYPE=iso9660 -o device)
                isodisksz=$(blockdev --getsz "$isodisk")
                # 0x8000 is the start of the Primary Volume Descriptor.
                # At offset 0x50 is the volume space size (a 32-bit
                # LSB integer), measured in 2048-byte logical blocks.
                # Multiply by 4 to get the number of 512-byte blocks.
                isopartend=$(( $(od -A n -t u4 -j $((0x8050)) -N 4 "$isodisk") * 4 ))
                echo "0 $isopartend linear $isodisk 0" | dmsetup create bootiso
                echo "0 $((isodisksz - isopartend)) linear $isodisk $isopartend" | dmsetup create persist
              }
              dropkick_dmsetup
            '';
            fileSystems = {
              # This filesystem is generated by dropkick and appended to the image.
              "/persist" = {
                device = "/dev/mapper/persist";
                fsType = "ext4";
                autoResize = true;
                neededForBoot = true;
              };
            } // lib.genAttrs [
              "/root"
              "/var/lib/caddy"
              "/var/log/caddy"
              "/var/log/journal"
            ]
              (dir: {
                device = "/persist${dir}";
                options = [ "bind" ];
              });
            # Ordinarily I would put this in /persist/etc/machine-id but systemd tries to create it before
            # /persist/etc is created elsewhere.
            environment.etc."machine-id".source = "/persist/machine-id";

            services.openssh = lib.mkIf dropkickInput.allowSsh {
              enable = true;
              settings.KbdInteractiveAuthentication = false;
              settings.PasswordAuthentication = false;
              settings.PermitRootLogin = "prohibit-password";
              hostKeys = [
                { path = "/persist/etc/ssh/ssh_host_rsa_key"; type = "rsa"; bits = 4096; }
                { path = "/persist/etc/ssh/ssh_host_ed25519_key"; type = "ed25519"; }
              ];
            };
            services.oxide-ssh-init.enable = dropkickInput.allowSsh;

            services.amazon-ssm-agent.enable = dropkickInput.allowAwsSsm;

            environment.systemPackages = lib.mkIf interactiveShell
              (with pkgs; [ htop tree vim ] ++ nixpkgsInput);

            # things for booting in EC2 and/or oxide
            # see also https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/virtualisation/amazon-image.nix
            boot.blacklistedKernelModules = [ "xen_fbfront" ];
            boot.extraModulePackages = [ config.boot.kernelPackages.ena ];
            boot.initrd.availableKernelModules = [ "nvme" "virtio_blk" "virtio_pci" "xen-blkfront" ];
            boot.kernelParams = [ "console=tty1" "console=ttyS0,115200n8" "random.trust_cpu=on" ];
            boot.loader.grub.extraConfig = ''
              serial --unit=0 --speed=115200 --word=8 --parity=no --stop=1
              terminal_output console serial
              terminal_input console serial
            '';
            boot.loader.timeout = lib.mkForce 1;
            systemd.services."serial-getty@ttyS0".enable = true;

            # https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/profiles/minimal.nix
            documentation.enable = interactiveShell;
            documentation.doc.enable = false;
            documentation.info.enable = false;
            documentation.man.enable = interactiveShell;
            documentation.nixos.enable = interactiveShell;
            fonts.fontconfig.enable = false;
            programs.command-not-found.enable = false;
            programs.less.lessopen = null;
            services.chrony.enable = true;
            services.resolved.enable = false;
            system.disableInstallerTools = true;
          })
        ];
      };

    };
}
