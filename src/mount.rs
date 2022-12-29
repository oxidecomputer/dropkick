use crate::command::{CommandExt, ExitStatusExt};
use crate::kpartx::Kpartx;
use anyhow::Result;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::TempDir;
use tokio::process::Command;
use tracing::{error, instrument};

#[derive(Debug)]
pub(crate) struct MountPoint<'a> {
    path: TempDir,
    kpartx: Option<Kpartx<'a>>,
}

impl<'a> MountPoint<'a> {
    pub(crate) async fn new(
        kpartx: Kpartx<'a>,
        tempdir_in: Option<&Path>,
    ) -> Result<MountPoint<'a>> {
        let path = match tempdir_in {
            Some(dir) => tempfile::tempdir_in(dir),
            None => tempfile::tempdir(),
        }?;
        Command::new("mount")
            .arg(kpartx.main_partition())
            .arg(path.path())
            .log()
            .status()
            .await?
            .check_status()?;
        Ok(MountPoint {
            path,
            kpartx: Some(kpartx),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        self.path.as_ref()
    }

    pub(crate) fn unmount(mut self) -> Result<Kpartx<'a>> {
        let kpartx = self.kpartx.take().unwrap();
        unmount(self.path.path())?;
        Ok(kpartx)
    }
}

impl Drop for MountPoint<'_> {
    #[instrument]
    fn drop(&mut self) {
        if self.kpartx.is_some() {
            if let Err(err) = unmount(self.path.path()) {
                error!(%err, "cleanup failed");
            }
        }
    }
}

fn unmount(path: &Path) -> Result<()> {
    StdCommand::new("umount")
        .arg(path)
        .log()
        .status()?
        .check_status()?;
    Ok(())
}
