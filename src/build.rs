use anyhow::{bail, ensure, Context, Result};
use askama::Template;
use camino::{Utf8Path, Utf8PathBuf};
use cargo_metadata::{MetadataCommand, Package};
use clap::Parser;
use iso9660::{DirectoryEntry, ISO9660};
use std::io::Read;
use std::process::Command;

const NIXOS_VERSION: &str = "22.11";

#[derive(Debug, Parser)]
pub(crate) struct Args {
    /// Allow SSH login (via SSH keys fetched by cloud-init)
    #[clap(long)]
    pub(crate) allow_login: bool,

    /// Specify which bin target to run in the image
    #[clap(long)]
    pub(crate) bin: Option<String>,

    /// Pass `--show-trace` to nix-build
    #[clap(long)]
    pub(crate) show_nix_trace: bool,

    /// Path to project directory (containing Cargo.toml)
    pub(crate) project_dir: Utf8PathBuf,
}

pub(crate) struct Output {
    pub(crate) image: Utf8PathBuf,
    pub(crate) nixos_version: String,
    pub(crate) package: Package,
    pub(crate) truncated_hash: String,
}

#[derive(Debug, Template)]
#[template(path = "nixos-config.nix", escape = "none")]
struct NixosConfig<'a> {
    allow_login: bool,
    bin_name: &'a str,
    nixos_version: &'static str,
    package: &'a Package,
    project_dir: Utf8PathBuf,
    toolchain_file: Option<Utf8PathBuf>,
}

pub(crate) fn build(args: &Args, tempdir: impl AsRef<Utf8Path>) -> Result<Output> {
    let tempdir = tempdir.as_ref();

    let metadata = MetadataCommand::new()
        .current_dir(&args.project_dir)
        .exec()
        .context("failed to run `cargo metadata`")?;
    let package = metadata
        .root_package()
        .context("failed to determine root package")?
        .clone();
    let mut bin_iter = package
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin"));
    let bin = match &args.bin {
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

    let toolchain_file = [
        args.project_dir.join("rust-toolchain"),
        args.project_dir.join("rust-toolchain.toml"),
    ]
    .into_iter()
    .find(|p| p.exists());

    let config_path = tempdir.join("config.nix");
    std::fs::write(
        &config_path,
        NixosConfig {
            allow_login: args.allow_login,
            bin_name: &bin.name,
            nixos_version: NIXOS_VERSION,
            package: &package,
            project_dir: args
                .project_dir
                .canonicalize_utf8()
                .context("failed to canonicalize project directory")?,
            toolchain_file,
        }
        .render()?,
    )?;

    let result_path = tempdir.join("result");
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
        .args(if args.show_nix_trace {
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

    let result_path = result_path
        .read_link_utf8()
        .context("failed to read result link")?;
    let truncated_hash = result_path
        .file_name()
        .and_then(|s| s.get(0..32))
        .context("failed to get truncated nix store hash for path")?
        .into();
    let original_image = result_path.join("iso").join("nixos.iso");

    let nixos_version = {
        let iso9660 =
            ISO9660::new(fs_err::File::open(&original_image)?).context("failed to read ISO")?;
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

    let image = tempdir.join("nixos.iso");
    fs_err::copy(&original_image, &image)?;

    Ok(Output {
        image,
        nixos_version,
        package,
        truncated_hash,
    })
}