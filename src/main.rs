#![warn(clippy::pedantic)]
#![allow(clippy::uninlined_format_args)]

mod build;
mod nix;
mod tempdir;

use crate::tempdir::Utf8TempDir;
use anyhow::{Context, Result};
use aws_sdk_ec2::model::{
    ArchitectureValues, BlockDeviceMapping, BootModeValues, EbsBlockDevice, VolumeType,
};
use camino::Utf8PathBuf;
use clap::Parser;
use coldsnap::{SnapshotUploader, SnapshotWaiter};
use env_logger::Env;
use indicatif::ProgressBar;

#[derive(Debug, Parser)]
enum Command {
    /// Build virtual machine image
    Build {
        #[clap(flatten)]
        build_args: crate::build::Args,

        /// Output path for built image
        output_path: Utf8PathBuf,
    },

    /// Create image for use in EC2
    CreateEc2Image {
        #[clap(flatten)]
        build_args: crate::build::Args,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("dropkick=info")).init();

    match Command::parse() {
        Command::Build {
            build_args,
            output_path,
        } => {
            let tempdir =
                Utf8TempDir::new_in(output_path.parent().context("output path has no parent")?)?;
            let output = crate::build::build(&build_args, &tempdir)?;
            fs_err::rename(output.image, output_path)?;
            Ok(())
        }
        Command::CreateEc2Image { build_args } => {
            let tempdir = Utf8TempDir::new()?;
            let output = crate::build::build(&build_args, &tempdir)?;
            let image_name_suffix = format!(
                "-{}-nixos{}-{}",
                output.package.version, output.nixos_version, output.truncated_hash
            );
            let image_name = format!(
                "{name:.len$}{suffix}",
                name = output.package.name,
                len = 128 - image_name_suffix.len(),
                suffix = image_name_suffix
            );

            let config = aws_config::load_from_env().await;
            let ebs_client = aws_sdk_ebs::Client::new(&config);
            let ec2_client = aws_sdk_ec2::Client::new(&config);

            log::info!("uploading EC2 snapshot");
            let snapshot_id = SnapshotUploader::new(ebs_client)
                .upload_from_file(
                    &output.image,
                    None,
                    Some(&image_name),
                    Some(ProgressBar::new(0)),
                )
                .await
                .context("failed to upload snapshot")?;
            log::info!(
                "uploaded EC2 snapshot ID {}; registering image",
                snapshot_id
            );

            SnapshotWaiter::new(ec2_client.clone())
                .wait_for_completed(&snapshot_id)
                .await
                .context("failed to wait for snapshot creation")?;
            let response = ec2_client
                .register_image()
                .name(&image_name)
                .virtualization_type("hvm")
                .architecture(ArchitectureValues::X8664)
                .boot_mode(BootModeValues::Uefi)
                .block_device_mappings(
                    BlockDeviceMapping::builder()
                        .device_name("/dev/xvda")
                        .ebs(
                            EbsBlockDevice::builder()
                                .snapshot_id(snapshot_id)
                                .volume_size(2)
                                .volume_type(VolumeType::Gp3)
                                .delete_on_termination(true)
                                .build(),
                        )
                        .build(),
                )
                .root_device_name("/dev/xvda")
                .ena_support(true)
                .sriov_net_support("simple")
                .send()
                .await?;
            println!(
                "{}",
                response
                    .image_id()
                    .context("no image ID in ec2:RegisterImage response")?
            );

            Ok(())
        }
    }
}
