use anyhow::{ensure, Context, Result};
use askama::Template;
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::Package;
use std::process::Command;

const NIXOS_VERSION: &str = "22.11";

#[derive(Debug)]
pub(crate) struct NixosBuilder<'a> {
    pub(crate) allow_login: bool,
    pub(crate) bin_name: &'a str,
    pub(crate) caddy_hostname: &'a str,
    pub(crate) package: &'a Package,
    pub(crate) project_dir: Utf8PathBuf,
    pub(crate) show_nix_trace: bool,
    pub(crate) toolchain_file: Option<Utf8PathBuf>,
}

#[derive(Debug, Template)]
#[template(path = "nixos-config.nix", escape = "none")]
struct NixosConfig<'a> {
    builder: &'a NixosBuilder<'a>,
    nixos_version: &'static str,
}

impl NixosBuilder<'_> {
    pub(crate) fn build(&self, tempdir: &Utf8Path) -> Result<Utf8PathBuf> {
        let config_path = tempdir.join("config.nix");
        let result_path = tempdir.join("result");

        std::fs::write(
            &config_path,
            NixosConfig {
                builder: self,
                nixos_version: NIXOS_VERSION,
            }
            .render()?,
        )?;

        log::info!("building image");
        let status = Command::new("nix-build")
            .args([
                "<nixpkgs/nixos>",
                "--argstr",
                "system",
                "x86_64-linux",
                "-A",
                "config.system.build.isoImage",
            ])
            .args(if self.show_nix_trace {
                &["--show-trace"][..]
            } else {
                &[]
            })
            .arg("--out-link")
            .arg(&result_path)
            .arg("-I")
            .arg(format!("nixpkgs=channel:nixos-{}", NIXOS_VERSION))
            .arg("-I")
            .arg(format!("nixos-config={}", config_path))
            .status()?;
        ensure!(status.success(), "nix-build failed with {}", status);

        result_path
            .read_link_utf8()
            .context("failed to read result link")
    }
}
