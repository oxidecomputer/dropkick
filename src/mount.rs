use crate::command::{CommandExt, ExitStatusExt};
use crate::kpartx::Kpartx;
use anyhow::Result;
use std::path::Path;
use std::process::Command as StdCommand;
use tempfile::TempDir;
use tokio::process::Command;
use tracing::{error, instrument};

#[derive(Debug)]
pub(crate) struct MountPoint {
    path: TempDir,
    kpartx: Option<Kpartx>,
}

impl MountPoint {
    pub(crate) async fn new(kpartx: Kpartx, tempdir_in: &Path) -> Result<MountPoint> {
        let path = tempfile::tempdir_in(tempdir_in)?;
        Command::new("mount")
            .arg(kpartx.main_partition())
            .arg(path.path())
            .with_sudo()
            .status()
            .await?
            .check_status()?;
        Ok(MountPoint {
            path,
            kpartx: Some(kpartx),
        })
    }

    pub(crate) fn unmount(mut self) -> Result<Kpartx> {
        let kpartx = self.kpartx.take().unwrap();
        unmount(self.path.path())?;
        Ok(kpartx)
    }
}

impl Drop for MountPoint {
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
        .with_sudo()
        .status()?
        .check_status()?;
    Ok(())
}
