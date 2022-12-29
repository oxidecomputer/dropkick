use crate::command::{CommandExt, ExitStatusExt};
use anyhow::{ensure, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tokio::process::Command;
use tracing::{error, instrument};

#[derive(Debug)]
pub(crate) struct Kpartx<'a> {
    source: &'a Path,
    partitions: Vec<String>,
}

impl Kpartx<'_> {
    pub(crate) async fn new(source: &Path) -> Result<Kpartx<'_>> {
        let output = Command::new("kpartx")
            .arg("-avs")
            .arg(source)
            .log()
            .output()
            .await
            .context("kpartx -avs failed")?;
        output.status.check_status()?;

        // define this here so that drop runs if we fail to parse the output
        let mut kpartx = Kpartx {
            source,
            partitions: Vec::new(),
        };

        for line in String::from_utf8(output.stdout)?.lines() {
            if let Some(s) = line.strip_prefix("add map ") {
                kpartx
                    .partitions
                    .push(s.split_whitespace().next().unwrap().to_owned());
            }
        }
        ensure!(
            !kpartx.partitions.is_empty(),
            "no partitions detected from kpartx output"
        );

        Ok(kpartx)
    }

    pub(crate) fn main_partition(&self) -> PathBuf {
        Path::new("/dev/mapper").join(&self.partitions[0])
    }

    pub(crate) fn delete(self) -> Result<()> {
        cleanup(self.source)?;
        Ok(())
    }
}

impl Drop for Kpartx<'_> {
    #[instrument]
    fn drop(&mut self) {
        if let Err(err) = cleanup(self.source) {
            error!(%err, "cleanup failed");
        }
    }
}

fn cleanup(source: &Path) -> Result<()> {
    StdCommand::new("kpartx")
        .arg("-d")
        .arg(source)
        .log()
        .status()?
        .check_status()?;
    Ok(())
}
