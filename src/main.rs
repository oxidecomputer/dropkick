use anyhow::Result;
use clap::Parser;
use dropkick::ImageContext;
use std::path::PathBuf;
use std::process::{ExitCode, Termination};
use tracing_subscriber::{prelude::*, EnvFilter};

#[derive(Debug, Parser)]
struct Options {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    Kick { output_file: PathBuf },
}

async fn run() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            EnvFilter::builder()
                .with_default_directive("dropkick=info".parse()?)
                .from_env()?,
        )
        .init();

    let options = Options::parse();
    match options.command {
        Command::Kick { output_file } => {
            let context = ImageContext::new(output_file).await?;
            context.finish()?;
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
