# Getting started with dropkick

Currently to build images you need an x86_64 Linux machine (a VM is fine) with [the Nix package manager](https://nixos.org/download.html) installed.

Once you have this, install dropkick:

```bash
cargo install --locked --git https://github.com/oxidecomputer/dropkick
```

## Building images

Supposing you have a project named `beachball` located in `~/git/beachball`, and you're planning to host it at `https://beachball.example`. Building an image might be as simple as:

```bash
dropkick build --hostname beachball.example ~/git/beachball output.img
```

> **Note**
> You can omit an output file to test that builds work.

This uses Nix to build a NixOS image that incorporates your project. If everything is successful, your image will be delivered to `output.img`.

`dropkick build` has a number of additional options. These are described in detail at [options.md](./options.md), but some of the more common ones are described below:

### Port numbers

dropkick expects services to listen on **port 8000** (`127.0.0.1:8000`) by default. You can specify a different port with `--port`:

```bash
dropkick build --port 12345 --hostname beachball.example ~/git/beachball
```

You can also specify this in your project's Cargo.toml:

```toml
[package.metadata.dropkick]
port = 12345
```

### Non-Rust dependencies

Your project is built within the Nix environment. If you have dependencies on non-Rust software, such as OpenSSL, you will need to tell dropkick which Nix packages to install. You can [use this tool to search nixpkgs](https://search.nixos.org/packages), and specify additional packages on the command line:

```bash
dropkick build --nixpkg pkg-config --nixpkg openssl --hostname beachball.example ~/git/beachball
```

or in your Cargo.toml:

```toml
[package.metadata.dropkick]
nixpkgs = ["pkg-config", "openssl"]
```

## Deploying images

See [aws.md](./aws.md) for getting running in AWS.

Oxide support soon&trade;!

## Cleaning up

dropkick is currently not good about cleaning up after itself, and over time the images in /nix/store will eat up more disk space. You can remove older images and build artifacts with `nix-collect-garbage`.
