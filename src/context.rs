use crate::kpartx::Kpartx;
use crate::mount::MountPoint;
use anyhow::{Context, Result};
use std::path::PathBuf;
use tempfile::NamedTempFile;

#[derive(Debug)]
#[must_use]
pub struct ImageContext {
    mount_point: MountPoint,
    output_path: PathBuf,
}

impl ImageContext {
    /// Download, verify, and unpack a disk image, creating a context to perform operations in.
    ///
    /// For now, this uses Ubuntu 22.04, but should eventually allow you to use a different version (or
    /// perhaps different distro altogether).
    pub async fn new(output_path: PathBuf) -> Result<ImageContext> {
        let output_dir = output_path
            .parent()
            .context("could not determine parent of output path")?;

        // decompress the image
        let image = NamedTempFile::new_in(output_dir)?.into_temp_path();
        crate::distro::fetch_ubuntu(None, &image).await?;

        let kpartx = Kpartx::new(image).await?;
        let mount_point = MountPoint::new(kpartx, output_dir).await?;

        Ok(ImageContext {
            mount_point,
            output_path,
        })
    }

    pub fn finish(self) -> Result<()> {
        let kpartx = self.mount_point.unmount()?;
        let image = kpartx.delete()?;
        image.persist(self.output_path)?;
        Ok(())
    }
}
