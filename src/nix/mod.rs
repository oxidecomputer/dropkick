// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::Package;
use serde::Serialize;
use std::process::Command;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NixosBuilder<'a> {
    #[serde(flatten)]
    pub(crate) build_args: &'a crate::build::Args,

    pub(crate) bin_name: &'a str,
    pub(crate) package: &'a Package,
    pub(crate) toolchain_file: Option<Utf8PathBuf>,
    pub(crate) workspace_root: Utf8PathBuf,
}

impl NixosBuilder<'_> {
    pub(crate) fn build(&self, tempdir: &Utf8Path) -> Result<Utf8PathBuf> {
        let json_path = tempdir.join("input.json");
        let result_path = tempdir.join("result");

        std::fs::write(tempdir.join("flake.nix"), include_str!("flake.nix"))?;
        std::fs::write(tempdir.join("flake.lock"), include_str!("flake.lock"))?;
        std::fs::write(json_path, serde_json::to_vec(&self)?)?;

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
            // TODO: allow specifying a nixpkgs commit instead of this
            .args(["--update-input", "nixpkgs"])
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
