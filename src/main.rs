use anyhow::Result;
use clap::Parser;
use std::process::{ExitCode, Termination};

#[derive(Debug, Parser)]
struct Options {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    Kick {},
}

async fn run() -> Result<()> {
    let options = Options::parse();
    match options.command {
        Command::Kick {} => {
            dropkick::distro::create_image().await?;
            Ok(())
        }
    }
}

// This is separate to wire up the ctrl-C signal handler so that `Drop` implementations get run on ctrl-C.
#[tokio::main]
async fn main() -> ExitCode {
    tokio::select! {
        ret = run() => ret.report(),
        _ = tokio::signal::ctrl_c() => 130.into(),
    }
}
