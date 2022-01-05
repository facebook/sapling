/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Result;
use futures::future;
use futures::stream::Stream;
use futures::stream::StreamExt;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio_stream::wrappers::LinesStream;
use tracing::error;
//use async_runtime::{block_on_future as block_on, stream_to_iter as block_on_stream};
use types::HgId;
use types::Key;
use types::RepoPathBuf;

// TODO(meyer): Find a better place for this. testutil? A debug command isn't really a test.
// Maybe refactor so less logic happens in hgcommands / pyrevisionstore, and migrate the actual
// business logic into revisionstore::scmstore::util or something.
pub async fn file_to_async_key_stream(path: PathBuf) -> Result<impl Stream<Item = Key>> {
    let file = BufReader::new(File::open(&path).await?);
    let lines = LinesStream::new(file.lines());
    Ok(lines
        .map(|line| {
            let line = line?;
            let hgid_path: Vec<_> = line.splitn(2, ',').collect();
            let hgid = HgId::from_str(hgid_path[0])?;
            let path = hgid_path
                .get(1)
                .ok_or_else(|| anyhow!("malformed line, no comma found"))?;
            let path = RepoPathBuf::from_string(path.to_string())?;
            anyhow::Ok(Key::new(path, hgid))
        })
        .filter_map(|res| {
            future::ready(match res {
                Ok(key) => Some(key),
                Err(e) => {
                    error!({ error = %e }, "error reading key from line");
                    None
                }
            })
        }))
}
