// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![warn(clippy::pedantic)]
#![allow(clippy::uninlined_format_args)]

mod build;
mod ec2;
mod nix;
mod tempdir;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use clap::Parser;
use env_logger::Env;
use tempfile::NamedTempFile;

#[derive(Debug, Parser)]
enum Command {
    /// Build virtual machine image
    Build {
        #[clap(flatten)]
        build_args: crate::build::Args,

        /// Output path for built image (if not specified, the output is deleted)
        output_path: Option<Utf8PathBuf>,
    },

    /// Create image for use in EC2
    CreateEc2Image {
        #[clap(flatten)]
        build_args: crate::build::Args,
    },

    #[clap(hide = true)]
    DumpNixInput {
        #[clap(flatten)]
        build_args: crate::build::Args,
    },
}

#[allow(clippy::too_many_lines)]
#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("dropkick=info")).init();

    match Command::parse() {
        Command::Build {
            build_args,
            output_path,
        } => {
            let (mut file, persist) = if let Some(output_path) = &output_path {
                let (file, temp_path) = NamedTempFile::new_in(
                    output_path.parent().context("output path has no parent")?,
                )?
                .into_parts();
                (file, Some((temp_path, output_path)))
            } else {
                (tempfile::tempfile()?, None)
            };
            build_args.create_iso(&mut file)?;
            if let Some((temp_path, output_path)) = persist {
                temp_path.persist(output_path)?;
            }
            Ok(())
        }
        Command::CreateEc2Image { build_args } => {
            let config = aws_config::load_from_env().await;
            let image_id = build_args.create_ec2_image(&config).await?;
            println!("{}", image_id);
            Ok(())
        }
        Command::DumpNixInput { build_args } => {
            println!("{}", build_args.nix_input_json()?);
            Ok(())
        }
    }
}
