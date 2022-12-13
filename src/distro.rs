use anyhow::{ensure, Context, Result};
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

#[derive(Debug)]
pub struct ImageContext {}

/// Download, verify, and unpack a disk image, creating a context to perform operations in.
///
/// For now, this uses Ubuntu 22.04, but should eventually allow you to use a different version (or
/// perhaps different distro altogether).
pub async fn create_image() -> Result<ImageContext> {
    let image_path = fetch_ubuntu(None).await?;
    todo!();
}

async fn fetch_ubuntu(serial: Option<&str>) -> Result<PathBuf> {
    // to make it easier to customize later...
    let version = "jammy";
    let arch = "amd64";

    // if no serial provided, look up the current serial
    let serial = match serial {
        Some("current") | None => reqwest::get(format!(
            "https://cloud-images.ubuntu.com/{}/current/unpacked/build-info.txt",
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
        "https://cloud-images.ubuntu.com/{}/{}/",
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

    let filename = format!("{}-server-cloudimg-{}.img", version, arch);
    let checksum = checksums
        .lines()
        .find_map(|line| line.strip_suffix(&format!(" *{}", filename)))
        .context("failed to find checksum in SHA256SUMS")?;

    let cache_dir = cache_dir()?;
    let cache_path = cache_dir.join(format!(
        "ubuntu-{version}-{arch}-{serial}.img",
        version = version,
        arch = arch,
        serial = serial
    ));
    if let Ok(mut file) = File::open(&cache_path).await {
        let mut hasher = Sha256::new();
        let mut buf = [0; 8192];
        loop {
            let n = file.read(&mut buf).await?;
            if n > 0 {
                hasher.update(&buf[..n]);
            } else {
                break;
            }
        }
        if hex::encode(hasher.finalize()) == checksum {
            return Ok(cache_path);
        } else {
            std::fs::remove_file(&cache_path)?;
        }
    }

    let mut response = reqwest::get(base_url.join(&filename)?).await?;
    let (file, temp_path) = NamedTempFile::new_in(&cache_dir)?.into_parts();
    let mut file = File::from_std(file);
    let mut hasher = Sha256::new();
    while let Some(chunk) = response.chunk().await? {
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    ensure!(
        hex::encode(hasher.finalize()) == checksum,
        "invalid checksum for downloaded image"
    );
    temp_path.persist(&cache_path)?;
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
