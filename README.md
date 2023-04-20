**dropkick** is some tooling for deploying a Rust HTTP service (perhaps using [dropshot](https://github.com/oxidecomputer/dropshot)) into the cloud.

At a high level dropkick's goal is to build your Rust project inside of a bootable, immutable Linux image running behind an HTTP reverse proxy, and help you get that image running in a cloud.

Non-goals currently include:
- scaling beyond one application server
- long-lived instaces with mutable state

See [docs/getting-started.md](./docs/getting-started.md) to get started with using dropkick.
