// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::Package;
use serde::Serialize;
use serde_json::Value;
use std::process::Command;

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

impl NixosBuilder {
    pub(crate) fn build(&self, tempdir: &Utf8Path) -> Result<Utf8PathBuf> {
        let mut flake_lock: Value = serde_json::from_str(include_str!("flake.lock"))?;
        if let Some(Value::Object(ref mut map)) = flake_lock.get_mut("nodes") {
            for item in REMOVE_FROM_FLAKE_LOCK {
                map.remove(*item);
            }

            if let Some(Value::Object(ref mut map)) =
                map.get_mut("root").and_then(|map| map.get_mut("inputs"))
            {
                for item in REMOVE_FROM_FLAKE_LOCK {
                    map.remove(*item);
                }
            }
        }

        let result_path = tempdir.join("result");

        std::fs::write(tempdir.join("flake.nix"), include_str!("flake.nix"))?;
        std::fs::write(
            tempdir.join("flake.lock"),
            serde_json::to_string(&flake_lock)?,
        )?;
        std::fs::write(tempdir.join("input.json"), serde_json::to_vec(&self)?)?;

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
                tempdir
            ))
            .status()?;
        ensure!(status.success(), "nix-build failed with {}", status);

        result_path
            .read_link_utf8()
            .context("failed to read result link")
    }
}
