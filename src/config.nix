{ config, lib, pkgs, modulesPath, ... }:
let
  dropkickInput = lib.importJSON ./input.json;
  dropshotServer = pkgs.callPackage
    ({ rustPlatform }:
      # This is a nix overlay commonly used to select a binary Rust release (in roughly the same way
      # rustup does):
      # https://github.com/NixOS/nixpkgs/blob/master/doc/languages-frameworks/rust.section.md#using-community-rust-overlays-using-community-rust-overlays
      with import <nixpkgs>
        {
          overlays = [
            (import (fetchTarball "https://github.com/oxalica/rust-overlay/archive/master.tar.gz"))
          ];
        };

      # Builds a rust package from a directory on disk.
      rustPlatform.buildRustPackage rec {
        src = nix-gitignore.gitignoreSource [ ] (/. + dropkickInput.projectDir);
        cargoLock = {
          lockFile = /. + dropkickInput.projectDir + "/Cargo.lock";
        };

        pname = dropkickInput.package.name;
        version = dropkickInput.package.version;

        nativeBuildInputs = [
          # Use a rust-toolchain(.toml) file with oxalica/rust-overlay (defined above) if we have one.
          # If we don't, use the latest stable.
          (if (isNull dropkickInput.toolchainFile)
          then rust-bin.stable.latest.minimal
          else (rust-bin.fromRustupToolchainFile (/. + dropkickInput.toolchainFile)))
        ];

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

  config = {
    system.stateVersion = dropkickInput.nixosVersion;

    systemd.services.dropshot-server = {
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig.ExecStart = "${dropshotServer}/bin/${dropkickInput.binName}";
      serviceConfig.Restart = "on-failure";
    };

    services.caddy = {
      enable = true;
      email = "iliana@oxide.computer";
      virtualHosts."${dropkickInput.caddyHostname}".extraConfig = ''
        reverse_proxy :8000
      '';
    };

    # The firewall is enabled by default. Enabling SSH automatically allows port 22 through the
    # firewall, but enabling Caddy does not allow any ports.
    networking.firewall = {
      enable = true;
      allowedTCPPorts = [ 80 443 ];
    };

    isoImage.appendToMenuLabel = "";
    isoImage.makeEfiBootable = true;
    # `makeUsbBootable` sets up the GPT label with the EFI system partition, which is necessary to
    # boot if a CD-ROM drive isn't being emulated.
    isoImage.makeUsbBootable = true;
    isoImage.squashfsCompression = "zstd -Xcompression-level 3";

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
  } // (if dropkickInput.allowLogin then {
    # enable sshd, and cloud-init to fetch ssh keys.
    # we specifically want cloud-init (despite its bulky closure) to support the cidata volume on oxide.
    services.openssh.enable = true;
    services.openssh.permitRootLogin = "prohibit-password";
    services.cloud-init.enable = true;
    services.cloud-init.network.enable = true;
    services.cloud-init.config = ''
      system_info:
        distro: nixos
        network:
          renderers: [ 'networkd' ]
      users:
        - root
      disable_root: false
      preserve_hostname: false

      cloud_init_modules:
        - update_hostname
        - users-groups
      cloud_config_modules:
        - ssh
      cloud_final_modules:
        - ssh-authkey-fingerprints
        - keys-to-console
        - final-message
    '';

    environment.systemPackages = with pkgs; [ htop ];
  } else {
    # https://github.com/NixOS/nixpkgs/blob/master/nixos/modules/profiles/minimal.nix
    documentation.enable = false;
    documentation.doc.enable = false;
    documentation.info.enable = false;
    documentation.man.enable = false;
    documentation.nixos.enable = false;
  });
}
