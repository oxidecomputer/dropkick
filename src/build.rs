// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::nix::Metadata;
use anyhow::{ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::MetadataCommand;
use clap::{Parser, ValueEnum};
use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom, Write};
use tempfile::NamedTempFile;

#[derive(Debug, Parser, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Args {
    /// Allow SSH login (via SSH keys fetched by cloud-init)
    #[clap(long)]
    pub(crate) allow_login: bool,

    /// Environment for the dropshot service (see EnvironmentFile in systemd.exec(5))
    #[clap(long)]
    pub(crate) env_file: Option<Utf8PathBuf>,

    /// Hostname the service will respond to
    #[clap(long)]
    pub(crate) hostname: String,

    /// Pass `--show-trace` to nix-build
    #[clap(long)]
    #[serde(skip_serializing)]
    pub(crate) show_nix_trace: bool,

    /// Configure Caddy to retrieve certificates from the Let's Encrypt staging environment (for
    /// testing Dropkick without hitting rate limits)
    #[clap(long)]
    pub(crate) test_cert: bool,

    /// Oxide only: Oxide Project
    #[clap(long)]
    pub(crate) oxide_project: Option<oxide::types::NameOrId>,

    /// Path to package directory (containing Cargo.toml)
    #[clap(default_value = ".")]
    #[serde(skip_serializing)]
    pub(crate) package_dir: Utf8PathBuf,

    /// Output path for built image (if not specified, the output is deleted)
    #[clap(long)]
    pub(crate) output_path: Option<Utf8PathBuf>,

    #[clap(flatten)]
    #[serde(flatten)]
    pub(crate) config: Config,
}

/// Options that can be set by command line args or by `[package.metadata.dropkick]`.
#[derive(Debug, Default, Clone, Parser, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "kebab-case", serialize = "camelCase"))]
pub(crate) struct Config {
    /// Specify which bin target to run in the image
    #[clap(long)]
    #[serde(skip_serializing)]
    pub(crate) bin: Option<String>,

    /// Where to store certificates
    #[clap(long)]
    pub(crate) cert_storage: Option<CertStorage>,

    /// Names of Nix packages to install during build and in the login environment
    #[clap(long = "nixpkg")]
    #[serde(default)]
    pub(crate) nixpkgs: Vec<String>,

    /// Port the service will listen on
    #[clap(long)]
    pub(crate) port: Option<u16>,

    /// Command line arguments to the dropshot service binary
    #[clap(long)]
    pub(crate) run_args: Option<String>,
}

#[derive(Debug, Clone, ValueEnum, Deserialize, Serialize)]
#[serde(rename_all(deserialize = "kebab-case"))]
pub(crate) enum CertStorage {
    /// Store certificates on the file system. Certificates will be lost on instance replacement.
    FileSystem,

    /// Store certificates in Amazon DynamoDB. This depends on resources created by the Dropkick
    /// CDK construct.
    Dynamodb,
}

impl Args {
    fn into_nixos_builder(mut self) -> Result<crate::nix::NixosBuilder> {
        self.package_dir = self
            .package_dir
            .canonicalize_utf8()
            .with_context(|| format!("failed to canonicalize {}", self.package_dir))?;

        let metadata = MetadataCommand::new()
            .current_dir(&self.package_dir)
            .exec()
            .context("failed to run `cargo metadata`")?;
        let mut package = metadata
            .root_package()
            .context(
                "cannot determine root package (does PACKAGE_DIR/Cargo.toml have a [package] entry?)",
            )?
            .clone();
        self.config = Config::from_metadata(&mut package.metadata)?.update(self.config);
        self.config.cert_storage = self.config.cert_storage.or(Some(CertStorage::FileSystem));
        self.config.port = self.config.port.or(Some(8000));
        self.config.run_args = self.config.run_args.or(Some(String::new()));
        if package.name == "dropkick" {
            log::warn!("you are attempting to build a dropkick image out of dropkick");
        }
        let mut bin_iter = package
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "bin"));
        let bin = match &self.config.bin {
            Some(bin) => bin_iter
                .find(|t| &t.name == bin)
                .with_context(|| format!("no bin target named {}", bin))?,
            None => {
                let bin = bin_iter.next().context("project contains no bin targets")?;
                ensure!(
                    bin_iter.next().is_none(),
                    "project contains multiple bin targets; choose one with --bin"
                );
                bin
            }
        };

        Ok(crate::nix::NixosBuilder {
            bin_name: bin.name.clone(),
            package,
            toolchain_file: find_toolchain_file(&self.package_dir, &metadata.workspace_root),
            workspace_root: metadata.workspace_root,

            build_args: self,
        })
    }

    pub(crate) fn nix_input_json(self) -> Result<String> {
        Ok(serde_json::to_string(&self.into_nixos_builder()?)?)
    }

    pub(crate) fn create_iso(self) -> Result<(tempfile::TempPath, Metadata)> {
        let output_path_arg = self.output_path.clone();
        let (mut file, temp_path) = if let Some(output_path) = &output_path_arg {
            NamedTempFile::new_in(output_path.parent().context("output path has no parent")?)?
                .into_parts()
        } else {
            NamedTempFile::new()?.into_parts()
        };

        let nixos_builder = self.into_nixos_builder()?;
        let metadata = nixos_builder.build(&mut file)?;

        // append an empty ext4 filesystem to the image (see notes about /persist in config.nix)
        sparse_copy(
            &mut zstd::Decoder::new(include_bytes!("fs/ext4.zst").as_slice())?,
            &mut file,
        )?;
        let len = file.stream_position()?;
        file.set_len(len)?;

        if let Some(output_path) = output_path_arg {
            std::fs::copy(&temp_path, output_path)?;
        }

        Ok((temp_path, metadata))
    }
}

impl Config {
    fn update(self, mut other: Config) -> Config {
        Config {
            bin: other.bin.or(self.bin),
            cert_storage: other.cert_storage.or(self.cert_storage),
            nixpkgs: {
                other.nixpkgs.extend(self.nixpkgs);
                other.nixpkgs
            },
            port: other.port.or(self.port),
            run_args: other.run_args.or(self.run_args),
        }
    }

    fn from_metadata(metadata: &mut serde_json::Value) -> Result<Config> {
        // Take the "dropkick" metadata out of the whole metadata value. If it
        // doesn't exist, return `Config::default()`.
        if let Some(metadata) = metadata.get_mut("dropkick") {
            Ok(serde_json::from_value(metadata.take())?)
        } else {
            Ok(Config::default())
        }
    }
}

fn find_toolchain_file(package_dir: &Utf8Path, workspace_root: &Utf8Path) -> Option<Utf8PathBuf> {
    fn inner(dir: &Utf8Path) -> Option<Utf8PathBuf> {
        ["rust-toolchain", "rust-toolchain.toml"]
            .into_iter()
            .find_map(|f| {
                let p = dir.join(f);
                p.exists().then_some(p)
            })
    }

    if package_dir.starts_with(workspace_root) {
        // Go up the directory tree until we find a toolchain file or hit `workspace_root`.
        for dir in package_dir.ancestors() {
            if !dir.starts_with(workspace_root) {
                break;
            }
            if let Some(path) = inner(dir) {
                return Some(path);
            }
        }
        None
    } else {
        // Only look in `package_dir` for a toolchain file.
        inner(package_dir)
    }
}

fn sparse_copy(src: &mut impl Read, dest: &mut (impl Write + Seek)) -> Result<()> {
    let mut buf = [0; 4096];
    let mut seek = 0;
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if buf[..n] == [0; 4096][..n] {
            seek += i64::try_from(n).unwrap();
        } else {
            if seek > 0 {
                dest.seek(SeekFrom::Current(seek))?;
                seek = 0;
            }
            dest.write_all(&buf)?;
        }
    }
    if seek > 0 {
        dest.seek(SeekFrom::Current(seek))?;
    }
    Ok(())
}
