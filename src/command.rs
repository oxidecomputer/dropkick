use std::ffi::OsStr;
use std::process::{Command, ExitStatus};
use tracing::info;

fn log_command(command: &Command) -> String {
    shell_words::join(
        std::iter::once(command.get_program())
            .chain(command.get_args())
            .map(OsStr::to_string_lossy),
    )
}

pub(crate) trait CommandExt {
    fn log(&mut self) -> &mut Self;
    fn with_sudo(&self) -> Self;
}

impl CommandExt for Command {
    fn log(&mut self) -> &mut Command {
        info!("running: {}", log_command(self));
        self
    }

    fn with_sudo(&self) -> Command {
        info!("running with sudo: {}", log_command(self));

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
    fn log(&mut self) -> &mut tokio::process::Command {
        info!("running: {}", log_command(self.as_std()));
        self
    }

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
