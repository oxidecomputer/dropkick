use crate::progress;
use anyhow::{ensure, Context, Result};
use indicatif::ProgressBar;
use pgp::armor::Dearmor;
use pgp::packet::PacketParser;
use pgp::Signature;
use reqwest::Url;
use sha2::{Digest, Sha256};
use std::io::Cursor;
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::warn;

/// Download and verify an Ubuntu cloud image, and uncompress it (using qemu-img) to `output_file`.
#[allow(clippy::too_many_lines)]
pub(crate) async fn fetch_ubuntu(serial: Option<&str>) -> Result<PathBuf> {
    // to make it easier to customize later...
    let version = "jammy";
    let arch = "amd64";

    let progress = ProgressBar::new_spinner()
        .with_message("fetching image information")
        .with_style(progress::running_style());

    // if no serial provided, look up the current serial
    let serial = match serial {
        Some("current") | None => reqwest::get(format!(
            "https://cloud-images.ubuntu.com/minimal/daily/{}/current/unpacked/build-info.txt",
            version
        ))
        .await?
        .text()
        .await?
        .lines()
        .find_map(|line| line.strip_prefix("serial=").map(str::to_owned))
        .context("no image serial found in current ubuntu image build info")?,
        Some(serial) => serial.to_owned(),
    };

    let base_url = Url::parse(&format!(
        "https://cloud-images.ubuntu.com/minimal/daily/{}/{}/",
        version, serial
    ))?;
    // fetch checksum file and its signature
    let checksums = reqwest::get(base_url.join("SHA256SUMS")?)
        .await?
        .text()
        .await?;
    let signature = parse_signature(
        &reqwest::get(base_url.join("SHA256SUMS.gpg")?)
            .await?
            .bytes()
            .await?,
    )?;
    signature.verify(&*crate::keys::UBUNTU, Cursor::new(checksums.as_bytes()))?;

    progress.set_style(progress::completed_style());
    progress.finish_with_message(format!("fetched image information (serial {})", serial));

    let filename = format!("{}-minimal-cloudimg-{}.img", version, arch);
    let checksum = hex::decode(
        checksums
            .lines()
            .find_map(|line| line.strip_suffix(&format!(" *{}", filename)))
            .context("failed to find checksum in SHA256SUMS")?,
    )
    .context("failed to hex decode checksum")?;

    let cache_dir = cache_dir()?;
    let cache_path = cache_dir.join(format!(
        "ubuntu-{version}-{arch}-{serial}.img",
        version = version,
        arch = arch,
        serial = serial
    ));
    let download_needed = match File::open(&cache_path).await {
        Ok(mut file) => {
            let progress = ProgressBar::new(file.metadata().await?.len())
                .with_message("verifying checksum")
                .with_style(progress::running_style());
            let mut hasher = Sha256::new();
            let mut buf = [0; 8192];
            loop {
                let n = file.read(&mut buf).await?;
                if n > 0 {
                    progress.inc(n as u64);
                    hasher.update(&buf[..n]);
                } else {
                    progress.finish();
                    break;
                }
            }
            if hasher.finalize().as_slice() == checksum {
                progress.set_style(progress::completed_style());
                progress.finish_with_message("verified checksum");
                false
            } else {
                progress.finish_with_message("checksum mismatch");
                warn!("cached image checksum mismatch, redownloading");
                std::fs::remove_file(&cache_path)?;
                true
            }
        }
        Err(_) => true,
    };

    if download_needed {
        let progress = ProgressBar::new(0)
            .with_message("downloading image")
            .with_style(progress::running_style());
        let mut response = reqwest::get(base_url.join(&filename)?).await?;
        if let Some(len) = response.content_length() {
            progress.set_length(len);
        }
        let (file, temp_path) = NamedTempFile::new_in(&cache_dir)?.into_parts();
        let mut file = File::from_std(file);
        let mut hasher = Sha256::new();
        while let Some(chunk) = response.chunk().await? {
            progress.inc(chunk.len().try_into().unwrap());
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        ensure!(
            hasher.finalize().as_slice() == checksum,
            "invalid checksum for downloaded image"
        );
        progress.set_style(progress::completed_style());
        progress.finish_with_message("downloaded image");
        temp_path.persist(&cache_path)?;
    }

    Ok(cache_path)
}

fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("failed to find user cache directory")?
        .join("dropkick")
        .join("images");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn parse_signature(signature: &[u8]) -> Result<Signature> {
    Ok(if signature.starts_with(b"-----") {
        PacketParser::new(Dearmor::new(Cursor::new(signature))).next()
    } else {
        PacketParser::new(Cursor::new(signature)).next()
    }
    .context("signature was empty")??
    .try_into()?)
}
