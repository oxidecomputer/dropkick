use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
struct Options {
    #[clap(subcommand)]
    command: Command,
}

#[derive(Debug, Parser)]
enum Command {
    Kick {},
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = Options::parse();
    match options.command {
        Command::Kick {} => {
            dropkick::distro::create_image().await?;
            Ok(())
        }
    }
}
