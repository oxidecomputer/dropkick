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
use tracing::{info, warn};

/// Download and verify an Ubuntu cloud image, and uncompress it (using qemu-img) to `output_file`.
pub(crate) async fn fetch_ubuntu(serial: Option<&str>) -> Result<PathBuf> {
    // to make it easier to customize later...
    let version = "jammy";
    let arch = "amd64";

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
    info!("current ubuntu image version: {}", serial);

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
    info!("verified signature of checksums file");

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
            let progress = ProgressBar::new(file.metadata().await?.len());
            progress.set_message("verifying checksum");
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
                info!("cached image checksum matches");
                false
            } else {
                warn!("cached image checksum mismatch, redownloading");
                std::fs::remove_file(&cache_path)?;
                true
            }
        }
        Err(_) => true,
    };

    if download_needed {
        let progress = ProgressBar::new(0);
        progress.set_message("downloading image");
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
