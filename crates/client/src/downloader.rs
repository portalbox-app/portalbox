use std::io::Write;
use std::{cmp::min, path::Path};

use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;

pub async fn download_file<P: AsRef<Path>>(url: &str, path: P) -> Result<(), anyhow::Error> {
    let client = Client::new();
    // Reqwest setup
    let res = client.get(url).send().await?;
    let total_size = res
        .content_length()
        .ok_or(anyhow::anyhow!("Failed to get content length"))?;

    // Indicatif setup
    let pb = ProgressBar::new(total_size);
    pb.set_style(ProgressStyle::default_bar()
        .template("{msg}\n{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
        .progress_chars("#>-"));
    pb.set_message("Downloading...");

    // download chunks
    let mut file = std::fs::File::create(path)?;
    let mut downloaded: u64 = 0;
    let mut stream = res.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk)?;
        let new = min(downloaded + (chunk.len() as u64), total_size);
        downloaded = new;
        pb.set_position(new);
    }

    pb.finish_with_message("Downloaded");
    return Ok(());
}
