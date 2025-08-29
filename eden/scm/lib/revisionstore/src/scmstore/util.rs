/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Result;
use anyhow::anyhow;
use futures::future;
use futures::stream::Stream;
use futures::stream::StreamExt;
use tokio::fs::File;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio_stream::wrappers::LinesStream;
use tracing::error;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

// TODO(meyer): Find a better place for this. testutil? A debug command isn't really a test.
// Maybe refactor so less logic happens in commands / pyrevisionstore, and migrate the actual
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

macro_rules! try_local_content {
    ($id:ident, $e:expr, $m:expr) => {
        if let Some(store) = $e.as_ref() {
            $m.requests.increment();
            $m.keys.increment();
            $m.singles.increment();
            match store.get_local_content_direct($id) {
                Ok(None) => {
                    $m.misses.increment();
                }
                Ok(Some(data)) => {
                    $m.hits.increment();
                    return Ok(Some(data.into()));
                }
                Err(err) => {
                    $m.errors.increment();
                    return Err(err);
                }
            }
        }
    };
}

pub(crate) use try_local_content;
