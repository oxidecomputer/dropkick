#![warn(clippy::pedantic)]

mod command;
mod distro;
mod keys;
mod kpartx;
mod mount;

use crate::kpartx::Kpartx;
use crate::mount::MountPoint;
use anyhow::{Context, Result};
// use aws_config::environment::EnvironmentVariableCredentialsProvider;
// use aws_sdk_ebs::{Client as EbsClient, Region};
use clap::Parser;
// use coldsnap::SnapshotUploader;
use command::{CommandExt, ExitStatusExt};
use indicatif::ProgressBar;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, Termination};
use tempfile::{NamedTempFile, TempPath};
use tracing::info;
use tracing_subscriber::{prelude::*, EnvFilter};

#[derive(Debug, Parser)]
struct Options {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    Build {
        #[clap(long)]
        output: PathBuf,
        #[clap(long)]
        tmpdir: Option<PathBuf>,

        dropshot_service: PathBuf,
    },

    /// Internal subcommand for doing image build steps that require root.
    #[clap(hide = true)]
    InternalBuild {
        #[clap(long)]
        image: PathBuf,
        #[clap(long)]
        dropshot_service: PathBuf,
        #[clap(long)]
        tmpdir: Option<PathBuf>,
    },

    CreateEc2Image {
        #[clap(long)]
        tmpdir: Option<PathBuf>,

        dropshot_service: PathBuf,
    },
}

async fn build_common(tmpdir: Option<&Path>, dropshot_service: &Path) -> Result<TempPath> {
    let input_image_path = crate::distro::fetch_ubuntu(None).await?;

    // We create this file in this process before sudoing so that it's owned by our user.
    let output_image_path = match tmpdir {
        Some(tmpdir) => NamedTempFile::new_in(tmpdir),
        None => NamedTempFile::new(),
    }?
    .into_temp_path();

    tokio::process::Command::new("qemu-img")
        .args(["convert", "-O", "raw"])
        .arg(&input_image_path)
        .arg(&output_image_path)
        .kill_on_drop(true)
        .status()
        .await
        .context("qemu-img convert failed")?
        .check_status()?;

    // We don't call `Command::kill_on_drop` here because we want the child process to be
    // able to clean up after itself. If a user sends Ctrl-C in a terminal, all processes in
    // the process group will receive SIGINT, and the `internal-build` process will clean up
    // after itself. This is the main use case for wanting to clean up.
    //
    // TODO(iliana): However, I think if a user sends SIGINT to the parent process another
    // way (e.g. kill(1)) then the child process will be orphaned.
    tokio::process::Command::new(std::env::current_exe()?)
        .arg("internal-build")
        .arg("--image")
        .arg(&output_image_path)
        .arg("--dropshot-service")
        .arg(dropshot_service)
        .with_sudo()
        .status()
        .await?
        .check_status()?;

    Ok(output_image_path)
}

async fn run() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive("dropkick=info".parse()?)
                .from_env()?,
        )
        .init();

    let options = Options::parse();
    match options.command {
        Command::Build {
            output,
            tmpdir,
            dropshot_service,
        } => {
            build_common(tmpdir.as_deref(), &dropshot_service)
                .await?
                .persist(output)?;
            Ok(())
        }

        Command::CreateEc2Image {
            tmpdir,
            dropshot_service,
        } => {
            let output_image_path = build_common(tmpdir.as_deref(), &dropshot_service).await?;

            let config = aws_config::load_from_env().await;
            let client = aws_sdk_ebs::Client::new(&config);
            let uploader = coldsnap::SnapshotUploader::new(client);

            info!("creating EC2 snapshot");
            let snapshot_id = uploader
                .upload_from_file(&output_image_path, None, None, Some(ProgressBar::new(0)))
                .await
                .context("failed to upload snapshot")?;
            info!("snapshot ID: {}", snapshot_id);

            Ok(())
        }

        Command::InternalBuild {
            image,
            dropshot_service,
            tmpdir,
        } => {
            // set this process's umask to 0022 to ensure files get written into the image as expected
            unsafe {
                libc::umask(0o022);
            }

            let kpartx = Kpartx::new(&image).await?;
            let mount_point = MountPoint::new(kpartx, tmpdir.as_deref()).await?;

            info!(
                "copying {} to /usr/local/bin/dropshot-service",
                dropshot_service.display()
            );
            tokio::fs::copy(
                dropshot_service,
                mount_point.path().join("usr/local/bin/dropshot-service"),
            )
            .await?;

            info!("writing dropshot.service unit");
            tokio::fs::write(
                mount_point
                    .path()
                    .join("etc/systemd/system/dropshot.service"),
                include_bytes!("dropshot.service"),
            )
            .await?;
            tokio::fs::symlink(
                "/etc/systemd/system/dropshot.service",
                mount_point
                    .path()
                    .join("etc/systemd/system/multi-user.target.wants/dropshot.service"),
            )
            .await?;

            let kpartx = mount_point.unmount()?;
            kpartx.delete()?;
            Ok(())
        }
    }
}

// This is separate to wire up the ctrl-C signal handler so that `Drop` implementations get run on ctrl-C.
#[tokio::main]
async fn main() -> ExitCode {
    tokio::select! {
        ret = run() => ret.report(),
        _ = tokio::signal::ctrl_c() => 130.into(),
    }
}
