// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{ensure, Context, Result};
use camino::Utf8PathBuf;
use cargo_metadata::Package;
use fs_err::File;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::Write;
use std::process::Command;

use crate::tempdir::Utf8TempDir;

// Flake inputs that we always want to keep up-to-date. We do this by removing their entries from
// flake.lock before writing it back out to a file.
// TODO: allow specifying a nixpkgs commit
const REMOVE_FROM_FLAKE_LOCK: &[&str] = &[
    "nixpkgs",      // ensure we have latest security backports
    "rust-overlay", // ensure we have the current stable release / recent nightlies
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NixosBuilder {
    #[serde(flatten)]
    pub(crate) build_args: crate::build::Args,

    pub(crate) bin_name: String,
    pub(crate) package: Package,
    pub(crate) toolchain_file: Option<Utf8PathBuf>,
    pub(crate) workspace_root: Utf8PathBuf,
}

#[derive(Debug)]
pub(crate) struct Metadata {
    pub(crate) flake_revs: HashMap<String, FlakeMetadata>,
    pub(crate) package: Package,
    pub(crate) store_hash: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FlakeMetadata {
    pub(crate) last_modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) rev: Option<String>,
}

impl NixosBuilder {
    pub(crate) fn build(self, writer: &mut impl Write) -> Result<Metadata> {
        let tempdir = Utf8TempDir::new()?;
        let flake_lock_path = tempdir.path().join("flake.lock");

        let mut flake_lock: FlakeLock = serde_json::from_str(include_str!("flake.lock"))?;
        for item in REMOVE_FROM_FLAKE_LOCK {
            flake_lock.nodes.remove(*item);
        }
        let root = flake_lock
            .nodes
            .get_mut(&flake_lock.root)
            .context("flake has no root node")?;
        for item in REMOVE_FROM_FLAKE_LOCK {
            root.inputs.remove(*item);
        }

        let result_path = tempdir.path().join("result");

        std::fs::write(tempdir.path().join("flake.nix"), include_str!("flake.nix"))?;
        std::fs::write(&flake_lock_path, serde_json::to_string(&flake_lock)?)?;
        std::fs::write(
            tempdir.path().join("input.json"),
            serde_json::to_vec(&self)?,
        )?;

        log::info!("building image");
        let status = Command::new("nix")
            .args([
                "--extra-experimental-features",
                "nix-command",
                "--extra-experimental-features",
                "flakes",
                "build",
                "--impure",
            ])
            .args(if self.build_args.show_nix_trace {
                &["--show-trace"][..]
            } else {
                &[]
            })
            .arg("--out-link")
            .arg(&result_path)
            .arg(format!(
                "path:{}#nixosConfigurations.dropkick.config.system.build.isoImage",
                tempdir.path()
            ))
            .status()?;
        ensure!(status.success(), "nix-build failed with {}", status);

        let result_path = result_path
            .read_link_utf8()
            .context("failed to read result link")?;
        std::io::copy(
            &mut File::open(result_path.join("iso").join("nixos.iso"))?,
            writer,
        )?;

        let mut flake_revs = HashMap::new();
        let flake_lock: FlakeLock =
            serde_json::from_str(&fs_err::read_to_string(flake_lock_path)?)?;
        for (flake_name, node) in flake_lock.nodes {
            if let Some(locked) = node.locked {
                flake_revs.insert(flake_name, locked.metadata);
            }
        }

        let store_hash = result_path
            .file_name()
            .and_then(|s| s.get(0..32))
            .context("failed to get truncated nix store hash for path")?
            .into();

        Ok(Metadata {
            flake_revs,
            package: self.package,
            store_hash,
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct FlakeLock {
    nodes: HashMap<String, FlakeNode>,
    root: String,
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FlakeNode {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    inputs: HashMap<String, Value>,
    locked: Option<FlakeLocked>,
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FlakeLocked {
    #[serde(flatten)]
    metadata: FlakeMetadata,
    #[serde(flatten)]
    extra: HashMap<String, Value>,
}
