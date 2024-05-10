// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::build::Args;
use anyhow::{anyhow, Result};
use base64::Engine;
use indicatif::{ProgressBar, ProgressStyle};
use oxide::config::Config;
use oxide::context::Context;
use oxide::types::ByteCount;
use oxide::types::DiskCreate;
use oxide::types::DiskSource;
use oxide::types::ExternalIpCreate;
use oxide::types::FinalizeDisk;
use oxide::types::ImageCreate;
use oxide::types::ImageSource;
use oxide::types::ImportBlocksBulkWrite;
use oxide::types::InstanceDiskAttachment;
use oxide::types::NameOrId;
use oxide::ClientDisksExt;
use oxide::ClientImagesExt;
use oxide::ClientInstancesExt;
use oxide::ClientSnapshotsExt;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

impl Args {
    pub(crate) async fn create_oxide_image(self, deploy: bool) -> Result<String> {
        let project = self
            .oxide_project
            .clone()
            .ok_or(anyhow!("Missing oxide project"))?;

        let hostname = self.hostname.clone();

        let (output_path, metadata) = self.create_iso()?;
        let mut image_name = format!(
            "{name:.len$}-{store_hash}",
            name = metadata.package.name,
            store_hash = metadata.store_hash,
            len = 128 - (32 + 1),
        )
        .replace("_", "-");
        image_name.truncate(63);
        log::info!("image name: {}", image_name);

        let config = Config::default();
        let context = Context::new(config).unwrap();

        if let Some(image) = context
            .client()?
            .image_list()
            .project(&project)
            .send()
            .await?
            .items
            .iter()
            .find(|x| *x.name == image_name)
        {
            log::info!("image already registered");
            return Ok(image.id.into());
        }

        log::info!("uploading Oxide snapshot");

        let mut disk_name = format!("{}-disk", &image_name);
        disk_name.truncate(63);

        let disk_size = get_disk_size(&output_path.to_path_buf())?;

        context
            .client()?
            .disk_create()
            .project(&project)
            .body(DiskCreate {
                name: disk_name.clone().try_into()?,
                description: format!("Dropkick {}", &image_name),
                disk_source: DiskSource::ImportingBlocks {
                    block_size: 512.try_into()?,
                },
                size: disk_size.into(),
            })
            .send()
            .await?;

        // Start the upload
        context
            .client()?
            .disk_bulk_write_import_start()
            .project(project.clone())
            .disk(disk_name.clone())
            .send()
            .await?;

        let mut file = File::open(&output_path)?;
        let mut offset = 0;
        let file_size = file.metadata()?.len();

        const CHUNK_SIZE: u64 = 512 * 1024;

        let pb = Arc::new(ProgressBar::new(file_size));
        pb.set_style(ProgressStyle::default_bar()
            .template("[{elapsed_precise}] [{wide_bar:.green}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")?);

        loop {
            let mut chunk = Vec::with_capacity(CHUNK_SIZE as usize);

            let n = file.by_ref().take(CHUNK_SIZE).read_to_end(&mut chunk)?;

            if n == 0 {
                break;
            }

            if !chunk.iter().all(|x| *x == 0) {
                let base64_encoded_data =
                    base64::engine::general_purpose::STANDARD.encode(&chunk[0..n]);

                context
                    .client()?
                    .disk_bulk_write_import()
                    .disk(disk_name.clone())
                    .project(project.clone())
                    .body(ImportBlocksBulkWrite {
                        offset,
                        base64_encoded_data,
                    })
                    .send()
                    .await?;
            }

            offset += CHUNK_SIZE;
            pb.inc(CHUNK_SIZE);
        }

        context
            .client()?
            .disk_bulk_write_import_stop()
            .project(project.clone())
            .disk(disk_name.clone())
            .send()
            .await?;

        let snapshot_name = format!("{}-snap", &image_name);

        context
            .client()?
            .disk_finalize_import()
            .project(project.clone())
            .disk(disk_name.clone())
            .body(FinalizeDisk {
                snapshot_name: Some(snapshot_name.clone().try_into()?),
            })
            .send()
            .await?;

        // Go from snapshot -> image
        let snapshot = context
            .client()?
            .snapshot_view()
            .project(project.clone())
            .snapshot(NameOrId::Name(snapshot_name.clone().try_into()?))
            .send()
            .await?;

        context
            .client()?
            .image_create()
            .project(project.clone())
            .body(ImageCreate {
                name: image_name.clone().try_into()?,
                description: format!("Dropkick {}", image_name),
                os: "NixOS".to_string(),
                version: "0.0.0".to_string(),
                source: ImageSource::Snapshot(snapshot.id),
            })
            .send()
            .await?;

        let imgs = context
            .client()?
            .image_list()
            .project(&project)
            .send()
            .await?;

        let img = imgs.items.iter().find(|x| *x.name == image_name).unwrap();

        if !deploy {
            return Ok(img.id.into());
        }

        let mut instance_disk_name = format!("{}-instance-disk", &image_name);
        instance_disk_name.truncate(63);

        let instance = context
            .client()?
            .instance_create()
            .project(&project)
            .body_map(|body| {
                body.name(image_name.clone())
                    .description(format!("Dropkick {}", &image_name))
                    .disks(vec![InstanceDiskAttachment::Create {
                        description: format!("Dropkick instance {}", &image_name),
                        disk_source: DiskSource::Image { image_id: img.id },
                        name: instance_disk_name.try_into().unwrap(),
                        size: ByteCount(1024 * 1024 * 1024 * 100),
                    }])
                    .external_ips(vec![ExternalIpCreate::Ephemeral { pool: None }])
                    .hostname(hostname)
                    .memory(ByteCount(1024 * 1024 * 1024 * 8))
                    .ncpus(4)
                    .start(true)
            })
            .send()
            .await?;

        // TODO adjust the firewall or print a message reminding people to do so?

        Ok(instance.id.into())
    }
}

// Borrowed from oxide.rs to give a disk size that Nexus will accept
fn get_disk_size(path: &PathBuf) -> Result<u64> {
    const ONE_GB: u64 = 1024 * 1024 * 1024;

    let disk_size = std::fs::metadata(path)?.len();

    // Nexus' disk size minimum is 1 GB, and Nexus only supports disks whose
    // size is a multiple of 1 GB
    let disk_size = if disk_size % ONE_GB != 0 {
        let rounded_down_gb: u64 = disk_size - disk_size % ONE_GB;
        assert_eq!(rounded_down_gb % ONE_GB, 0);
        rounded_down_gb + ONE_GB
    } else {
        disk_size
    };

    Ok(disk_size)
}
