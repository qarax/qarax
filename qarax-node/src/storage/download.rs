use std::{error::Error, io::Cursor, path::Path};

use reqwest::header::{HeaderValue, RANGE};
use tokio::{fs::OpenOptions, io};

// TODO make configurable chunk size
// TODO handle errors and create specific erros

const CHUNK_SIZE: u64 = 10 * 1024 * 1024;

pub async fn download(
    url: &str,
    path: &Path,
    size: u64,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    // TODO validate path exisence
    let mut file = OpenOptions::new().write(true).open(&path).await?;

    let client = reqwest::Client::new();
    for range in PartialRangeIter::new(0, size - 1, CHUNK_SIZE)? {
        let response = client.get(url).header(RANGE, range).send().await?;
        // TODO check status code and report errors!
        let mut response = Cursor::new(response.bytes().await?);
        io::copy(&mut response, &mut file).await?;
    }

    Ok(())
}

struct PartialRangeIter {
    start: u64,
    end: u64,
    buffer_size: u64,
}

impl PartialRangeIter {
    pub fn new(
        start: u64,
        end: u64,
        buffer_size: u64,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if buffer_size == 0 {
            Err("invalid buffer_size, give a value greater than zero.")?;
        }
        Ok(PartialRangeIter {
            start,
            end,
            buffer_size,
        })
    }
}

impl Iterator for PartialRangeIter {
    type Item = HeaderValue;
    fn next(&mut self) -> Option<Self::Item> {
        if self.start > self.end {
            None
        } else {
            let prev_start = self.start;
            self.start += std::cmp::min(self.buffer_size as u64, self.end - self.start + 1);
            Some(
                HeaderValue::from_str(&format!("bytes={}-{}", prev_start, self.start - 1)).unwrap(),
            )
        }
    }
}
