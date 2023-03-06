# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

{ config, lib, pkgs, modulesPath, ... }:
let
  applyPkgs = list: map (s: builtins.getAttr s pkgs) list;
  dropkickInput = lib.importJSON ./input.json;
  dropshotServer = pkgs.callPackage
    ({ rustPlatform }:
      with import <nixpkgs>
        {
          overlays = [
            # This is a nix overlay commonly used to select a binary Rust release (in roughly the
            # same way rustup does):
            # https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/rust.section.md#using-community-rust-overlays-using-community-rust-overlays
            (import (fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz"))
          ];
        };
      rustPlatform.buildRustPackage {
        src = nix-gitignore.gitignoreSource [ ] (/. + dropkickInput.workspaceRoot);
        cargoLock = {
          lockFile = /. + dropkickInput.workspaceRoot + "/Cargo.lock";
        };

        pname = dropkickInput.package.name;
        version = dropkickInput.package.version;

        nativeBuildInputs = [
          # Use a rust-toolchain(.toml) file with oxalica/rust-overlay (defined above) if we have one.
          # If we don't, use the latest stable.
          (if (dropkickInput.toolchainFile != null)
          then (rust-bin.fromRustupToolchainFile (/. + dropkickInput.toolchainFile))
          else rust-bin.stable.latest.minimal)
        ] ++ applyPkgs dropkickInput.buildDeps;
        buildInputs = applyPkgs dropkickInput.deps;

        # Disable `cargo test`.
        doCheck = false;
      }
    )
    { };
in
{
  imports = [
    (modulesPath + "/installer/cd-dvd/iso-image.nix")
  ];

  config = lib.recursiveUpdate
    {
      system.stateVersion = dropkickInput.nixosVersion;

      systemd.services.dropshot-server = {
        wantedBy = [ "multi-user.target" ];
        after = [ "network.target" ];
        before = [ "caddy.service" ];
        serviceConfig = {
          ExecStart = "${dropshotServer}/bin/${dropkickInput.binName} ${dropkickInput.runArgs}";
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

      # Tell dhcpcd to wait for an IPv4 address, so that IMDS is reachable if we're in AWS.
      networking.dhcpcd.wait = "ipv4";

      systemd.services.dropkick-ssh-keys = {
        description = "Add SSH keys from EC2 IMDS or the Oxide cidata volume";
        wantedBy = [ "multi-user.target" ];
        after = [ "network-online.target" ];
        wants = [ "network-online.target" ];
        before = [ "sshd.service" ];

        script = ''
          [[ -f /root/.ssh/authorized_keys ]] && exit 0
          umask 0077
          mkdir /root/.ssh

          if [[ $(${pkgs.dmidecode}/bin/dmidecode --string system-uuid) == ec2* ]]; then
            token=$(${pkgs.curl}/bin/curl -v --retry-all-errors --retry 5 --retry-delay 2 --fail --connect-timeout 1 \
              -X PUT -H 'X-aws-ec2-metadata-token-ttl-seconds: 600' http://169.254.169.254/latest/api/token)
            ${pkgs.curl}/bin/curl -H "X-aws-ec2-metadata-token: $token" -o /root/.ssh/authorized_keys \
              http://169.254.169.254/latest/meta-data/public-keys/0/openssh-key
          elif [[ -b /dev/disk/by-label/cidata ]]; then
            ${pkgs.mtools}/bin/copy -i /dev/disk/by-label/cidata ::/meta-data - \
              | ${pkgs.jq}/bin/jq -r '."public-keys"[]' > /root/.ssh/authorized_keys
          fi
        '';

        serviceConfig.Type = "oneshot";
        serviceConfig.RemainAfterExit = true;
      };

      environment.systemPackages = with pkgs; [ htop helix tree vim ] ++ applyPkgs dropkickInput.install;
    } else {
      # https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/profiles/minimal.nix
      documentation.enable = false;
      documentation.doc.enable = false;
      documentation.info.enable = false;
      documentation.man.enable = false;
      documentation.nixos.enable = false;
    });
}
