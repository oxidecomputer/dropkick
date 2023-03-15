# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-22.11";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";
    crane.inputs.rust-overlay.follows = "rust-overlay";
    nixie-tubes.url = "github:oxidecomputer/nixie-tubes";
  };

  outputs = { nixpkgs, rust-overlay, crane, nixie-tubes, ... }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { };

      dropkickInput = pkgs.lib.importJSON ./input.json;
      nixpkgsInput = map (s: builtins.getAttr s pkgs) dropkickInput.nixpkgs;
    in
    rec {

      packages."${system}".default =
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

      nixosConfigurations.dropkick = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          nixie-tubes.nixosModules.ssh-init

          ({ config, lib, pkgs, modulesPath, ... }:
            {
              imports = [
                (modulesPath + "/installer/cd-dvd/iso-image.nix")
              ];

              config = lib.recursiveUpdate
                {
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
                    } // (if (dropkickInput.envFile != null) then {
                      EnvironmentFile = pkgs.copyPathToStore (/. + dropkickInput.envFile);
                    } else { });
                  };

                  services.caddy = {
                    enable = true;
                    email = "iliana@oxide.computer";

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
                        interval 2m
                        burst 5
                      }

                      # disable the zerossl issuer
                      cert_issuer acme
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
                  } // (if dropkickInput.testCert then {
                    acmeCA = "https://acme-staging-v02.api.letsencrypt.org/directory";
                  } else { });

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
                  # `makeUsbBootable` sets up the GPT label with the EFI system partition, which is necessary to
                  # boot if a CD-ROM drive isn't being emulated.
                  isoImage.makeUsbBootable = true;
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
                        local isopart isodisk isodisksz isopartend
                        isopart=$(blkid -t TYPE=iso9660 -o device)
                        isodisk=/dev/$(basename "$(readlink -f "/sys/class/block/$(basename "$isopart")/..")")
                        isodisksz=$(blockdev --getsz "$isodisk")
                        isopartend=$(blockdev --getsz "$isopart")
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
                  fonts.fontconfig.enable = false;
                  programs.command-not-found.enable = false;
                  services.chrony.enable = true;
                  services.resolved.enable = false;
                }
                (if dropkickInput.allowLogin then {
                  services.openssh = {
                    enable = true;
                    kbdInteractiveAuthentication = false;
                    passwordAuthentication = false;
                    permitRootLogin = "prohibit-password";
                    hostKeys = [
                      { path = "/persist/etc/ssh/ssh_host_rsa_key"; type = "rsa"; bits = 4096; }
                      { path = "/persist/etc/ssh/ssh_host_ed25519_key"; type = "ed25519"; }
                    ];
                  };

                  environment.systemPackages = with pkgs; [ htop helix tree vim ] ++ nixpkgsInput;
                } else {
                  services.oxide-ssh-init.enable = false;

                  # https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/profiles/minimal.nix
                  documentation.enable = false;
                  documentation.doc.enable = false;
                  documentation.info.enable = false;
                  documentation.man.enable = false;
                  documentation.nixos.enable = false;
                });
            })
        ];
      };

    };
}

