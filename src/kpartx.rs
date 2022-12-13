use crate::command::{CommandExt, ExitStatusExt};
use anyhow::{ensure, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use tempfile::TempPath;
use tokio::process::Command;
use tracing::{error, instrument};

#[derive(Debug)]
pub(crate) struct Kpartx {
    source: Option<TempPath>,
    partitions: Vec<String>,
}

impl Kpartx {
    pub(crate) async fn new(source: TempPath) -> Result<Kpartx> {
        let output = Command::new("kpartx")
            .arg("-avs")
            .arg(&source)
            .with_sudo()
            .output()
            .await
            .context("kpartx -avs failed")?;
        output.status.check_status()?;

        // define this here so that drop runs if we fail to parse the output
        let mut kpartx = Kpartx {
            source: Some(source),
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

    pub(crate) fn delete(mut self) -> Result<TempPath> {
        let source = self.source.take().unwrap();
        cleanup(&source)?;
        Ok(source)
    }
}

impl Drop for Kpartx {
    #[instrument]
    fn drop(&mut self) {
        if let Some(source) = &self.source {
            if let Err(err) = cleanup(source) {
                error!(%err, "cleanup failed");
            }
        }
    }
}

fn cleanup(source: &Path) -> Result<()> {
    StdCommand::new("kpartx")
        .arg("-d")
        .arg(source)
        .with_sudo()
        .status()?
        .check_status()?;
    Ok(())
}
