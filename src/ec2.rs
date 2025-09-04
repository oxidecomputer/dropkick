// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::build::Args;
use anyhow::{Context, Result};
use aws_config::SdkConfig;
use aws_sdk_ebs::types::Tag;
use aws_sdk_ec2::types::{
    ArchitectureValues, BlockDeviceMapping, BootModeValues, EbsBlockDevice, Filter,
    ImdsSupportValues, VolumeType,
};
use coldsnap::{SnapshotUploader, SnapshotWaiter, UploadZeroBlocks};
use indicatif::ProgressBar;
use tempfile::NamedTempFile;

impl Args {
    pub(crate) async fn create_ec2_image(self, config: &SdkConfig) -> Result<String> {
        let (mut file, temp_path) = NamedTempFile::new()?.into_parts();
        let metadata = self.create_iso(&mut file)?;
        let image_name = format!(
            "{name:.len$}-{store_hash}",
            name = metadata.package.name,
            store_hash = metadata.store_hash,
            len = 128 - (32 + 1),
        );
        log::info!("image name: {}", image_name);

        let ebs_client = aws_sdk_ebs::Client::new(config);
        let ec2_client = aws_sdk_ec2::Client::new(config);

        if let Some(image_id) = ec2_client
            .describe_images()
            .owners("self")
            .filters(Filter::builder().name("name").values(&image_name).build())
            .send()
            .await?
            .images()
            .first()
            .and_then(|image| image.image_id())
        {
            log::info!("image already registered");
            return Ok(image_id.into());
        }

        let mut tags = vec![
            tag("package.name", metadata.package.name),
            tag("package.version", metadata.package.version.to_string()),
            tag("store_hash", metadata.store_hash),
        ];
        for (flake_name, metadata) in metadata.flake_revs {
            let modified_tag = tag(
                format!("flake.{flake_name}.last_modified").as_str(),
                metadata.last_modified.to_string(),
            );
            tags.push(modified_tag);

            if let Some(rev) = metadata.rev {
                let rev_tag = tag(format!("flake.{flake_name}.rev").as_str(), rev);
                tags.push(rev_tag);
            }
        }

        log::info!("uploading EC2 snapshot");
        let snapshot_id = SnapshotUploader::new(ebs_client)
            .upload_from_file(
                &temp_path,
                None,
                Some(&image_name),
                Some(tags),
                Some(ProgressBar::new(0)),
                Some(UploadZeroBlocks::Include),
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
            .imds_support(ImdsSupportValues::V20)
            .send()
            .await?;
        let image_id = response
            .image_id()
            .context("no image ID in ec2:RegisterImage response")?;

        Ok(image_id.into())
    }
}

fn tag(key: &str, value: String) -> Tag {
    Tag::builder()
        .key(format!("dropkick:{key}"))
        .value(value)
        .build()
}
