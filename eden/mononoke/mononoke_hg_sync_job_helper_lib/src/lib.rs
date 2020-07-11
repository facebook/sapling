// (c) Facebook, Inc. and its affiliates. Confidential and proprietary.

#![deny(warnings)]

use anyhow::{bail, format_err, Error, Result};
use blobstore::{Blobstore, Loadable};
use bytes_old::{Bytes as BytesOld, BytesMut as BytesMutOld};
use cloned::cloned;
use context::CoreContext;
use futures::{future::try_join_all, stream::TryStreamExt};
use futures_ext::FutureExt;
use futures_old::{
    future::{loop_fn, IntoFuture, Loop},
    Future,
};
use mercurial_bundles::stream_start;
use mononoke_types::RawBundle2Id;
use slog::{info, Logger};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tempfile::NamedTempFile;
use tokio::{
    fs::{read as async_read_all, File as AsyncFile, OpenOptions},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    time::{delay_for, timeout},
};
use tokio_io::codec::Decoder;

pub async fn save_bundle_to_temp_file<B: Blobstore + Clone>(
    ctx: &CoreContext,
    blobstore: &B,
    bundle2_id: RawBundle2Id,
) -> Result<NamedTempFile, Error> {
    let tempfile = NamedTempFile::new()?;

    save_bundle_to_file(
        ctx,
        blobstore,
        bundle2_id,
        tempfile.path().to_path_buf(),
        false, /* create */
    )
    .await?;

    Ok(tempfile)
}

pub async fn save_bundle_to_file<B: Blobstore + Clone>(
    ctx: &CoreContext,
    blobstore: &B,
    bundle2_id: RawBundle2Id,
    file: PathBuf,
    create: bool,
) -> Result<(), Error> {
    let bytes = bundle2_id.load(ctx.clone(), blobstore).await?;
    save_bytes_to_file(bytes.into_bytes(), file, create).await?;

    Ok(())
}

pub async fn save_bytes_to_temp_file<B: AsRef<[u8]>>(bytes: B) -> Result<NamedTempFile, Error> {
    let tempfile = NamedTempFile::new()?;
    save_bytes_to_file(
        bytes,
        tempfile.path().to_path_buf(),
        false, /* create */
    )
    .await?;
    Ok(tempfile)
}

pub async fn save_bytes_to_file<B: AsRef<[u8]>>(
    bytes: B,
    file: PathBuf,
    create: bool,
) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .create(create)
        .write(true)
        .open(file)
        .await?;

    file.write_all(bytes.as_ref()).await?;
    file.flush().await?;

    Ok(())
}

pub async fn write_to_named_temp_file<B>(bytes: B) -> Result<NamedTempFile, Error>
where
    B: AsRef<[u8]>,
{
    let tempfile = NamedTempFile::new()?;
    let mut file = open_tempfile(&tempfile).await?;

    file.write_all(bytes.as_ref()).await?;
    file.flush().await?;

    Ok(tempfile)
}

async fn open_tempfile(tempfile: &NamedTempFile) -> Result<AsyncFile, Error> {
    let file = OpenOptions::new()
        .write(true)
        .open(tempfile.path().to_path_buf())
        .await?;

    Ok(file)
}

/// Get lines after the first `num` lines in file
pub async fn lines_after(p: impl AsRef<Path>, num: usize) -> Result<Vec<String>, Error> {
    let file = AsyncFile::open(p).await?;
    let reader = BufReader::new(file);
    let mut v: Vec<_> = reader.lines().try_collect().await?;
    Ok(v.split_off(num))
}

/// Wait until the file has more than `initial_num` lines, then return new lines
/// Timeout after `timeout_millis` ms.
pub async fn wait_till_more_lines(
    p: impl AsRef<Path>,
    initial_num: usize,
    timeout_millis: u64,
) -> Result<Vec<String>, Error> {
    let p = p.as_ref().to_path_buf();

    let read = async {
        loop {
            let new_lines = lines_after(p.clone(), initial_num).await?;
            let new_num = new_lines.len();
            let stop = new_num > 0;
            if stop {
                return Ok(new_lines);
            }

            delay_for(Duration::from_millis(100)).await;
        }
    };

    match timeout(Duration::from_millis(timeout_millis), read).await {
        Ok(Ok(lines)) => Ok(lines),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(Error::msg("timed out waiting for new lines")),
    }
}

pub async fn merge_timestamp_files(
    _ctx: &CoreContext,
    timestamp_files: &[PathBuf],
) -> Result<NamedTempFile, Error> {
    //TODO(ikostia): implement an async version of this
    let maybe_current_contents: Result<Vec<Vec<u8>>> = timestamp_files
        .iter()
        .map(|path| {
            let mut file = std::fs::File::open(path)
                .map_err(|e| format_err!("Error opening timestamps file {:?}: {}", path, e))?;
            let mut file_bytes = vec![];
            let read_result = file
                .read_to_end(&mut file_bytes)
                .map_err(|e| format_err!("Failed reading timestamps file {:?}: {}", path, e));
            // let's safeguard ourselves from the missing last newline
            if file_bytes.len() > 0 && file_bytes[file_bytes.len() - 1] != b'\n' {
                file_bytes.push(b'\n');
            }
            read_result.map(|_sz| file_bytes)
        })
        .collect();

    match maybe_current_contents {
        Err(e) => Err(e),
        Ok(current_contents) => {
            let merged_contents: Vec<u8> = current_contents.into_iter().flatten().collect();
            write_to_named_temp_file(merged_contents).await
        }
    }
}

pub async fn merge_bundles(
    _ctx: &CoreContext,
    bundles: &[PathBuf],
) -> Result<NamedTempFile, Error> {
    let bundle_contents = try_join_all(bundles.iter().map(|path| async_read_all(path))).await?;
    let merged = merge_bundle_contents(bundle_contents)?;
    write_to_named_temp_file(merged).await
}

fn merge_bundle_contents(bundle_contents: Vec<Vec<u8>>) -> Result<BytesOld> {
    if bundle_contents.is_empty() {
        bail!("no bundles provided");
    }

    if bundle_contents.len() == 1 {
        return Ok(BytesOld::from(bundle_contents.get(0).cloned().unwrap()));
    }

    let len = bundle_contents.len();
    let mut merged_content = BytesMutOld::new();
    for (i, content) in bundle_contents.into_iter().enumerate() {
        let content = BytesMutOld::from(content);
        if i == 0 {
            merged_content.extend_from_slice(&strip_suffix(content)?);
        } else if i == len - 1 {
            merged_content.extend_from_slice(&strip_prefix(content)?);
        } else {
            merged_content.extend_from_slice(&strip_suffix(strip_prefix(content)?)?);
        }
    }

    Ok(merged_content.freeze())
}

fn strip_prefix(bytes: BytesMutOld) -> Result<BytesMutOld> {
    let mut bytes = bytes;
    let mut start_decoder = stream_start::StartDecoder {};
    // StartDecoder strips header and stream parameters
    let stream_params = start_decoder
        .decode(&mut bytes)?
        .ok_or(Error::msg("bundle header not found"))?;

    let compression = stream_params.m_stream_params.get("compression");
    if !(compression.is_none() || compression == Some(&"UN".to_string())) {
        bail!("cannot concatenate compressed bundles");
    }

    Ok(bytes)
}

fn strip_suffix(bytes: BytesMutOld) -> Result<BytesMutOld> {
    let mut bytes = bytes;
    if bytes.len() < 4 {
        bail!("bundle is too small!");
    }
    let last_bytes = bytes.split_off(bytes.len() - 4);
    if last_bytes != &"\0\0\0\0" {
        bail!("unexpected bundle suffix");
    }
    Ok(bytes)
}

pub fn read_file_contents<F: Seek + Read>(f: &mut F) -> Result<String> {
    // NOTE: Normally (for our use case at this time), we don't advance our position in this file,
    // but let's be conservative and seek to the start anyway.
    let pos = SeekFrom::Start(0);
    f.seek(pos)
        .map_err(|e| format_err!("could not seek to {:?}: {:?}", pos, e))?;

    let mut buff = vec![];
    f.read_to_end(&mut buff)
        .map_err(|e| format_err!("could not read: {:?}", e))?;

    String::from_utf8(buff).map_err(|e| format_err!("log file is not valid utf-8: {:?}", e))
}

#[derive(Copy, Clone)]
pub struct RetryAttemptsCount(pub usize);

pub fn retry<V, Fut, Func>(
    logger: Logger,
    func: Func,
    base_retry_delay_ms: u64,
    retry_num: usize,
) -> impl Future<Item = (V, RetryAttemptsCount), Error = Error>
where
    V: Send + 'static,
    Fut: Future<Item = V, Error = Error>,
    Func: Fn(usize) -> Fut + Send + 'static,
{
    use tokio_timer::Delay;

    loop_fn(1, move |attempt| {
        cloned!(logger);
        func(attempt)
            .and_then(move |res| Ok(Loop::Break(Ok((res, RetryAttemptsCount(attempt))))))
            .or_else({
                move |err| {
                    if attempt >= retry_num {
                        Ok(Loop::Break(Err(err))).into_future().left_future()
                    } else {
                        info!(
                            logger.clone(),
                            "retrying attempt {} of {}...",
                            attempt + 1,
                            retry_num
                        );

                        let delay =
                            Duration::from_millis(base_retry_delay_ms * 2u64.pow(attempt as u32));
                        Delay::new(Instant::now() + delay)
                            .and_then(move |_| Ok(Loop::Continue(attempt + 1)))
                            .map_err(|e| -> Error { e.into() })
                            .right_future()
                    }
                }
            })
    })
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_ext::FutureExt;
    use mercurial_bundles::bundle2::{Bundle2Stream, StreamEvent};
    use mercurial_bundles::Bundle2Item;

    use futures_old::{future, Stream};
    use slog::{o, Discard};
    use std::io::Cursor;

    #[test]
    fn test_simple_merge_bundle() {
        let res = b"HG20\x00\x00\x00\x0eCompression\x3dUN\x00\x00\x00\x12\x0bPHASE-HEADS\x00\x00\x00\x00\x00\x00\x00\x00\x00H\x00\x00\x00\x00bbbbbbbbbbbbbbbbbbbb\x00\x00\x00\x00cccccccccccccccccccc\x00\x00\x00\x01aaaaaaaaaaaaaaaaaaaa\x00\x00\x00\x00\x00\x00\x00\x00";
        let merged_bundle = merge_bundle_contents(vec![res.to_vec(), res.to_vec()]);
        assert!(merged_bundle.is_ok());
        let merged_bundle = merged_bundle.unwrap();

        let cursor = Cursor::new(merged_bundle);
        let logger = Logger::root(Discard, o!());
        Bundle2Stream::new(logger, cursor)
            .collect()
            .wait()
            .expect("failed to create bundle2stream");

        let merged_bundle = merge_bundle_contents(vec![]);
        assert!(merged_bundle.is_err());
    }

    #[test]
    fn test_fail_if_compressed() {
        let res = b"HG20\x00\x00\x00\x0eCompression\x3dBZ\x00\x00\x00\x12\x0bPHASE-HEADS\x00\x00\x00\x00\x00\x00\x00\x00\x00H\x00\x00\x00\x00bbbbbbbbbbbbbbbbbbbb\x00\x00\x00\x00cccccccccccccccccccc\x00\x00\x00\x01aaaaaaaaaaaaaaaaaaaa\x00\x00\x00\x00\x00\x00\x00\x00";
        let merged_bundle = merge_bundle_contents(vec![res.to_vec(), res.to_vec()]);
        assert!(merged_bundle.is_err());
    }

    #[test]
    fn test_fail_if_wrong_suffix() {
        let res = b"HG20\x00\x00\x00\x0eCompression\x3dUN\x00\x00\x00\x12\x0bPHASE-HEADS\x00\x00\x00\x00\x00\x00\x00\x00\x00H\x00\x00\x00\x00bbbbbbbbbbbbbbbbbbbb\x00\x00\x00\x00cccccccccccccccccccc\x00\x00\x00\x01aaaaaaaaaaaaaaaaaaaa\x00\x00\x00\x00\x00\x00\x00\x01";
        let merged_bundle = merge_bundle_contents(vec![res.to_vec(), res.to_vec()]);
        assert!(merged_bundle.is_err());
    }

    fn parse_bundle_stream(s: Bundle2Stream<Cursor<BytesOld>>) -> usize {
        let f = s
            .filter_map(|stream_event| match stream_event {
                StreamEvent::Next(i) => Some(i),
                StreamEvent::Done(_) => None,
            })
            .and_then(|bundle2item| match bundle2item {
                Bundle2Item::Start(_) => future::ok(()).boxify(),
                Bundle2Item::Replycaps(_, fut) => fut.map(|_| ()).boxify(),
                Bundle2Item::B2xCommonHeads(_, stream) => stream.collect().map(|_| ()).boxify(),
                Bundle2Item::B2xRebasePack(_, stream) => stream.collect().map(|_| ()).boxify(),
                Bundle2Item::B2xRebase(_, stream) => stream.collect().map(|_| ()).boxify(),
                _ => panic!("unexpected bundle2 item"),
            })
            .collect();

        let items = f.wait();
        assert!(items.is_ok());
        items.unwrap().len()
    }

    #[test]
    fn test_real_bundle() {
        let bundle: &[u8] = &include_bytes!("pushrebase_replay.bundle")[..];
        let merged_bundle = merge_bundle_contents(vec![bundle.to_vec(), bundle.to_vec()]);
        assert!(merged_bundle.is_ok());
        let merged_bundle = merged_bundle.unwrap();
        let logger = Logger::root(Discard, o!());

        let cursor = Cursor::new(merged_bundle);
        let bundle_stream = Bundle2Stream::new(logger.clone(), cursor);
        assert_eq!(parse_bundle_stream(bundle_stream), 9);

        let merged_bundle =
            merge_bundle_contents(vec![bundle.to_vec(), bundle.to_vec(), bundle.to_vec()]);
        let cursor = Cursor::new(merged_bundle.unwrap());
        let bundle_stream = Bundle2Stream::new(logger.clone(), cursor);
        assert_eq!(parse_bundle_stream(bundle_stream), 13);
    }
}
