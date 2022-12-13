use std::ffi::OsStr;
use std::process::{Command, ExitStatus};
use tracing::info;

pub(crate) trait CommandExt {
    fn with_sudo(&self) -> Self;
}

impl CommandExt for Command {
    fn with_sudo(&self) -> Command {
        info!(
            "running with sudo: {}",
            shell_words::join(
                std::iter::once(self.get_program())
                    .chain(self.get_args())
                    .map(OsStr::to_string_lossy),
            )
        );

        let mut command = Command::new("sudo");
        command.arg(self.get_program());
        command.args(self.get_args());
        if let Some(dir) = self.get_current_dir() {
            command.current_dir(dir);
        }
        for (k, v) in self.get_envs() {
            if let Some(v) = v {
                command.env(k, v);
            } else {
                command.env_remove(k);
            }
        }

        command
    }
}

impl CommandExt for tokio::process::Command {
    fn with_sudo(&self) -> tokio::process::Command {
        self.as_std().with_sudo().into()
    }
}

pub(crate) trait ExitStatusExt {
    fn check_status(&self) -> std::io::Result<()>;
}

impl ExitStatusExt for ExitStatus {
    fn check_status(&self) -> std::io::Result<()> {
        if self.success() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("process exited with {}", self),
            ))
        }
    }
}
