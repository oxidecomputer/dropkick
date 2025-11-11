// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![warn(clippy::pedantic)]
#![allow(clippy::uninlined_format_args)]

mod build;
mod ec2;
mod nix;
mod oxide;
mod tempdir;

use anyhow::{bail, Context, Result};
use aws_config::BehaviorVersion;
use aws_sdk_cloudformation::types::{Capability, Parameter, StackStatus};
use clap::Parser;
use env_logger::Env;
use std::time::Duration;

#[derive(Debug, Parser)]
enum Command {
    /// Build virtual machine image
    Build {
        #[clap(flatten)]
        build_args: crate::build::Args,
    },

    /// Create image for use in EC2
    CreateEc2Image {
        #[clap(flatten)]
        build_args: crate::build::Args,
    },
    /// Create image for use in Oxide
    CreateOxideImage {
        #[clap(flatten)]
        build_args: crate::build::Args,
    },
    /// Deploy an image to Oxide
    DeployOxideImage {
        #[clap(flatten)]
        build_args: crate::build::Args,
    },

    /// Deploy a new image to an existing CloudFormation stack
    DeployEc2Image {
        #[clap(flatten)]
        build_args: crate::build::Args,

        /// CloudFormation stack name
        stack_name: String,
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
        Command::Build { build_args } => {
            build_args.create_iso()?;

            Ok(())
        }
        Command::CreateEc2Image { build_args } => {
            let config = aws_config::load_defaults(BehaviorVersion::v2025_08_07()).await;
            let image_id = build_args.create_ec2_image(&config).await?;
            println!("{}", image_id);
            Ok(())
        }
        Command::CreateOxideImage { build_args } => {
            let id = build_args.create_oxide_image(false).await?;
            println!("{}", id);
            Ok(())
        }
        Command::DeployOxideImage { build_args } => {
            let id = build_args.create_oxide_image(true).await?;
            println!("image ID: {}", id);

            Ok(())
        }
        Command::DeployEc2Image {
            build_args,
            stack_name,
        } => {
            let config = aws_config::load_defaults(BehaviorVersion::v2025_08_07()).await;
            let image_id = build_args.create_ec2_image(&config).await?;
            log::info!("image ID: {}", image_id);

            let client = aws_sdk_cloudformation::Client::new(&config);
            client
                .update_stack()
                .stack_name(&stack_name)
                .use_previous_template(true)
                // The `@oxide/dropkick-cdk` construct creates an IAM instance
                // role no matter what, so we always need this capability.
                .capabilities(Capability::CapabilityIam)
                .parameters(
                    Parameter::builder()
                        .parameter_key("DropkickImageId")
                        .parameter_value(image_id)
                        .build(),
                )
                .send()
                .await?;
            log::info!("stack update in progress, waiting for result");
            // https://github.com/boto/botocore/blob/5d22dbbb9e8d29e2bd43146df6e3954a7a74a44c/botocore/data/cloudformation/2010-05-15/waiters-2.json#L125
            // waiting for up to an hour seems ridiculous for dropkick stacks,
            // so let's do max 15 minutes, 15 second interval = 60 attempts
            for _ in 0..60 {
                tokio::time::sleep(Duration::from_secs(15)).await;
                let response = client
                    .describe_stacks()
                    .stack_name(&stack_name)
                    .send()
                    .await?;
                let status = response
                    .stacks()
                    .first()
                    .context("no stacks returned in cloudformation:DescribeStacks")?
                    .stack_status()
                    .context("no stack status")?;
                match status {
                    StackStatus::UpdateComplete => {
                        log::info!("stack updated successfully");
                        return Ok(());
                    }
                    StackStatus::UpdateFailed
                    | StackStatus::UpdateRollbackFailed
                    | StackStatus::UpdateRollbackComplete => {
                        bail!("stack update failed: {:?}", status);
                    }
                    _ => {}
                }
            }
            bail!("timed out waiting for stack update");
        }
        Command::DumpNixInput { build_args } => {
            println!("{}", build_args.nix_input_json()?);
            Ok(())
        }
    }
}
