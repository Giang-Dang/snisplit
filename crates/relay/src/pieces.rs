use std::io;
use std::iter;
use std::ops::Range;
use std::time::Duration;

use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::time::sleep;

pub async fn write_pieces<W: AsyncWrite + Unpin>(
    writer: &mut W,
    bytes: &[u8],
    cuts: &[usize],
    gap: Duration,
) -> io::Result<usize> {
    let mut written = 0;
    for piece in piece_ranges(bytes.len(), cuts) {
        if written > 0 {
            sleep(gap).await;
        }
        writer.write_all(&bytes[piece]).await?;
        writer.flush().await?;
        written += 1;
    }
    Ok(written)
}

fn piece_ranges(len: usize, cuts: &[usize]) -> Vec<Range<usize>> {
    let mut ranges = Vec::with_capacity(cuts.len() + 1);
    let mut start = 0;
    for &end in cuts.iter().chain(iter::once(&len)) {
        if start < end && end <= len {
            ranges.push(start..end);
            start = end;
        }
    }
    ranges
}

#[cfg(test)]
mod tests;
