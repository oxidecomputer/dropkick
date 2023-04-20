# Getting started with dropkick

Currently to build images you need an x86_64 Linux machine (a VM is fine) with [the Nix package manager](https://nixos.org/download.html) installed.

Once you have this, install dropkick:

```bash
cargo install --locked --git https://github.com/oxidecomputer/dropkick
```

## Building images

Supposing you have a project named `beachball` located in `~/git/beachball`. Building an image might be as simple as:

```bash
dropkick build ~/git/beachball output.img
```

## Deploying images

See [aws.md](./aws.md) for getting running in AWS.

Oxide support soon&trade;!
