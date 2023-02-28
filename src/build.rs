use anyhow::{bail, ensure, Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{MetadataCommand, Package};
use clap::Parser;
use fs_err::File;
use iso9660::{DirectoryEntry, ISO9660};
use serde::Serialize;
use std::io::Read;

#[derive(Debug, Parser, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Args {
    /// Allow SSH login (via SSH keys fetched by cloud-init)
    #[clap(long)]
    pub(crate) allow_login: bool,

    /// Specify which bin target to run in the image
    #[clap(long)]
    #[serde(skip_serializing)]
    pub(crate) bin: Option<String>,

    /// Environment for the dropshot service (see EnvironmentFile in systemd.exec(5))
    #[clap(long)]
    pub(crate) env_file: Option<Utf8PathBuf>,

    /// Hostname the service will respond to
    #[clap(long)]
    pub(crate) hostname: String,

    /// Port the service will listen on
    #[clap(long, default_value = "8000")]
    pub(crate) port: u16,

    /// Pass `--show-trace` to nix-build
    #[clap(long)]
    #[serde(skip_serializing)]
    pub(crate) show_nix_trace: bool,

    /// Configure Caddy to retrieve certificates from the Let's Encrypt staging environment (for
    /// testing Dropkick without hitting rate limits)
    #[clap(long)]
    pub(crate) test_cert: bool,

    /// Path to project directory (containing Cargo.toml)
    #[clap(default_value = ".")]
    pub(crate) project_dir: Utf8PathBuf,
}

pub(crate) struct Output {
    pub(crate) image: Utf8PathBuf,
    pub(crate) nixos_version: String,
    pub(crate) package: Package,
    pub(crate) truncated_hash: String,
}

impl Args {
    pub(crate) fn build(&self, tempdir: impl AsRef<Utf8Path>) -> Result<Output> {
        let tempdir = tempdir.as_ref();

        let metadata = MetadataCommand::new()
            .current_dir(&self.project_dir)
            .exec()
            .context("failed to run `cargo metadata`")?;
        let package = metadata
            .root_package()
            .context("failed to determine root package")?
            .clone();
        if package.name == "dropkick" {
            log::warn!("you are attempting to build a dropkick image out of dropkick");
        }
        let mut bin_iter = package
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "bin"));
        let bin = match &self.bin {
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

        let result_path = crate::nix::NixosBuilder {
            build_args: self,
            bin_name: &bin.name,
            package: &package,
            toolchain_file: ["rust-toolchain", "rust-toolchain.toml"]
                .into_iter()
                .find_map(|f| {
                    let p = self.project_dir.join(f);
                    p.exists().then_some(p)
                }),
        }
        .build(tempdir)?;

        let truncated_hash = result_path
            .file_name()
            .and_then(|s| s.get(0..32))
            .context("failed to get truncated nix store hash for path")?
            .into();
        let iso_image = result_path.join("iso").join("nixos.iso");

        let nixos_version = {
            let iso9660 = ISO9660::new(File::open(&iso_image)?).context("failed to read ISO")?;
            if let Some(DirectoryEntry::File(file)) = iso9660
                .open("version.txt")
                .context("failed to open version.txt")?
            {
                let mut s = String::new();
                file.read()
                    .read_to_string(&mut s)
                    .context("failed to read version.txt")?;
                s.trim().to_owned()
            } else {
                bail!("version.txt is not a file in the ISO");
            }
        };

        let final_image = tempdir.join("nixos.img");
        fs_err::copy(iso_image, &final_image)?;

        Ok(Output {
            image: final_image,
            nixos_version,
            package,
            truncated_hash,
        })
    }
}
