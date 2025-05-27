/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use async_trait::async_trait;
use auto_impl::auto_impl;
use bytes::Bytes;
use git_types::ObjectContent;
use gix_hash::ObjectId;
use gix_object::Commit;
use gix_object::Kind;
use gix_object::ObjectRef;
use gix_object::Tag;
use gix_object::Tree;
use mononoke_macros::mononoke;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

type ObjectSender = oneshot::Sender<Result<ObjectContent>>;

#[async_trait]
#[auto_impl(&, Arc, Box)]
pub trait GitReader: Clone + Send + Sync + 'static {
    async fn get_object(&self, oid: &gix_hash::oid) -> Result<ObjectContent>;

    async fn read_tag(&self, oid: &gix_hash::oid) -> Result<Tag> {
        let object = self.get_object(oid).await?;
        object
            .parsed
            .try_into_tag()
            .map_err(|_| format_err!("{} is not a tag", oid))
    }

    async fn read_commit(&self, oid: &gix_hash::oid) -> Result<Commit> {
        let object = self.get_object(oid).await?;
        object
            .parsed
            .try_into_commit()
            .map_err(|_| format_err!("{} is not a commit", oid))
    }

    async fn read_tree(&self, oid: &gix_hash::oid) -> Result<Tree> {
        let object = self.get_object(oid).await?;
        object
            .parsed
            .try_into_tree()
            .map_err(|_| format_err!("{} is not a tree", oid))
    }

    async fn read_raw_object(&self, oid: &gix_hash::oid) -> Result<Bytes> {
        self.get_object(oid)
            .await
            .map(|obj| obj.raw)
            .with_context(|| format!("Error while fetching Git object for ID {}", oid))
    }

    async fn peel_to_commit(&self, mut oid: ObjectId) -> Result<Option<ObjectId>> {
        let mut object = self.get_object(&oid).await?.parsed;
        while let Some(tag) = object.as_tag() {
            oid = tag.target;
            object = self.get_object(&oid).await?.parsed;
        }

        Ok(object.as_commit().map(|_| oid))
    }

    async fn is_annotated_tag(&self, object_id: &ObjectId) -> Result<bool> {
        Ok(self
            .get_object(object_id)
            .await
            .with_context(|| {
                format_err!(
                    "Failed to fetch git object {} for checking if its a tag",
                    object_id,
                )
            })?
            .parsed
            .as_tag()
            .is_some())
    }
}

/// Uses `git-cat-file` to read a git repository's ODB directly
#[derive(Clone)]
pub struct GitRepoReader {
    send_request: mpsc::Sender<ObjectId>,
    outstanding_requests: Arc<Mutex<HashMap<ObjectId, Vec<ObjectSender>>>>,
}

impl GitRepoReader {
    /// Create a new repo reader for the repo at `repo_path`, using `git_command_path`
    /// as `git`
    pub async fn new(git_command_path: &Path, repo_path: &Path) -> Result<Self> {
        let mut batch_cat_file = Command::new(git_command_path)
            .current_dir(repo_path)
            .env_clear()
            // We expect dropping to close stdin, which will cause git cat-file
            // to quit
            .kill_on_drop(false)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .arg("cat-file")
            .arg("--batch")
            .arg("--unordered")
            .spawn()?;

        let outstanding_requests: Arc<Mutex<HashMap<ObjectId, _>>> =
            Arc::new(Mutex::new(HashMap::new()));

        {
            let outstanding_requests = outstanding_requests.clone();
            mononoke::spawn_task(read_objects_task(
                outstanding_requests,
                batch_cat_file
                    .stdout
                    .take()
                    .context("stdout not set up properly")?,
            ))
        };

        // Channel is used so that we don't have issues around ownership of stdin in `get_object`
        let (send_request, mut recv_request) = mpsc::channel(1);
        {
            let outstanding_requests = outstanding_requests.clone();
            let mut cat_file_stdin = batch_cat_file
                .stdin
                .take()
                .context("stdin not set up properly")?;
            mononoke::spawn_task(async move {
                while let Some(object) = recv_request.recv().await {
                    if cat_file_stdin
                        .write_all(format!("{}\n", object).as_bytes())
                        .await
                        .is_err()
                    {
                        recv_request.close();
                        // stdin is gone - we can't continue
                        let mut outstanding_requests =
                            outstanding_requests.lock().expect("lock poisoned");
                        for (_, queue) in outstanding_requests.drain() {
                            for sender in queue {
                                let _ = sender.send(Err(anyhow!("git cat-file stdin closed")));
                            }
                        }
                    }
                }
            })
        };

        Ok(Self {
            send_request,
            outstanding_requests,
        })
    }

    /// Read `oid` from the git store
    pub fn get_object(
        &self,
        oid: &gix_hash::oid,
    ) -> impl Future<Output = Result<ObjectContent>> + use<> {
        let outstanding_requests = self.outstanding_requests.clone();
        let send_request = self.send_request.clone();
        let oid = oid.to_owned();
        async move {
            let permit = send_request
                .reserve()
                .await
                .context("get_object: failed to send request")?;
            let (sender, recv) = oneshot::channel();
            outstanding_requests
                .lock()
                .expect("lock poisoned")
                .entry(oid.to_owned())
                .or_default()
                .push(sender);

            permit.send(oid);

            recv.await.context("get_object: received an error")?
        }
    }
}

#[async_trait]
impl GitReader for GitRepoReader {
    async fn get_object(&self, oid: &gix_hash::oid) -> Result<ObjectContent> {
        self.get_object(oid).await
    }
}

fn parse_kind_and_size(header: &str) -> Result<(Kind, usize)> {
    if header == "missing" {
        bail!("Object is missing");
    }
    let (kind, size) = header
        .split_once(' ')
        .context("Expected object kind and size")?;

    Ok((kind_str_to_kind(kind)?, size.trim_end().parse()?))
}

fn kind_str_to_kind(kind: &str) -> Result<Kind> {
    let kind = match kind {
        "tree" => Kind::Tree,
        "blob" => Kind::Blob,
        "commit" => Kind::Commit,
        "tag" => Kind::Tag,
        _ => bail!("Object kind {} unknown", kind),
    };
    Ok(kind)
}

fn parse_cat_header(header: &str) -> Result<(ObjectId, Result<(Kind, usize)>)> {
    let (oid, content_type) = header.split_once(' ').context("No git object id")?;
    Ok((oid.parse()?, parse_kind_and_size(content_type)))
}

fn convert_to_object(kind: Kind, size: usize, bytes: Vec<u8>) -> Result<ObjectContent> {
    let object_ref = ObjectRef::from_bytes(kind, &bytes).with_context(|| {
        format!(
            "Failed to parse:\n```\n{}\n```\ninto object of kind {:?}",
            String::from_utf8_lossy(&bytes),
            kind
        )
    })?;
    let mut raw = format!("{} {}\x00", kind, size).into_bytes();
    raw.append(&mut bytes.clone());

    Ok(ObjectContent {
        parsed: object_ref.into_owned(),
        raw: Bytes::from(raw),
    })
}

async fn read_objects_task(
    outstanding_requests: Arc<Mutex<HashMap<ObjectId, Vec<ObjectSender>>>>,
    cat_file_stdout: ChildStdout,
) -> Result<()> {
    let mut reader = BufReader::new(cat_file_stdout);
    let mut buf = String::new();

    while reader.read_line(&mut buf).await? > 0 {
        // buf contains either "oid missing" for a missing object or "oid type size" for a present object
        // If it doesn't give us an ObjectId, all we can do is loop on until we find a header again.
        if let Ok((oid, maybe_details)) = parse_cat_header(&buf) {
            // If no-one is waiting on this ObjectId, we can't report the object state,
            // but still need to follow protocol to clear it out.
            let maybe_sender = {
                // Take at most one waiter - we send the request once for each reader
                // so we'll get it re-read from disk for the next one in the queue
                let mut outstanding_requests = outstanding_requests.lock().expect("lock poisoned");
                outstanding_requests.remove(&oid).and_then(|mut queue| {
                    let item = queue.pop();
                    if !queue.is_empty() {
                        outstanding_requests.insert(oid.clone(), queue);
                    }
                    item
                })
            };

            // If the header doesn't indicate an object, we send the error onwards and loop again
            let (kind, size) = match maybe_details {
                Ok(d) => d,
                Err(e) => {
                    if let Some(sender) = maybe_sender {
                        let _ = sender.send(
                            Err(e)
                                .with_context(|| format!("read_objects_task failed for {}", &buf)),
                        );
                    }
                    continue;
                }
            };

            // We have an object. Let's try and read it.

            // We need to read size bytes, and then send it on as an object to unblock our listener
            let mut bytes: Vec<u8> = vec![0; size];
            reader
                .read_exact(&mut bytes)
                .await
                .with_context(|| format!("failed to read exactly {} bytes", size))?;
            if let Some(sender) = maybe_sender {
                let object = convert_to_object(kind, size, bytes);
                let _ = sender.send(object);
            }
            // Finally, there's an empty line after the object, but before the next header. Consume it
            reader
                .read_line(&mut buf)
                .await
                .context("expected an empty line after the object but before the next header")?;
            buf.clear();
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_kind_str_to_kind() {
        assert!(kind_str_to_kind("forest").is_err());
        assert_eq!(kind_str_to_kind("blob").unwrap(), Kind::Blob);
        assert_eq!(kind_str_to_kind("commit").unwrap(), Kind::Commit);
        assert_eq!(kind_str_to_kind("tag").unwrap(), Kind::Tag);
        assert_eq!(kind_str_to_kind("tree").unwrap(), Kind::Tree);
    }

    #[mononoke::test]
    fn test_parse_kind_and_size() {
        assert!(parse_kind_and_size("missing").is_err());
        assert_eq!(
            parse_kind_and_size("tree 12345").unwrap(),
            (Kind::Tree, 12345)
        );
    }

    #[mononoke::test]
    fn test_parse_cat_header() {
        assert!(parse_cat_header("I am a fish").is_err());
        let (oid, kind_and_size) =
            parse_cat_header("99cd00206e418c5fb0e9bd885ded84b8781194b7 tag 682\n").unwrap();
        assert_eq!(
            oid,
            ObjectId::from_hex(b"99cd00206e418c5fb0e9bd885ded84b8781194b7").unwrap()
        );
        assert_eq!(kind_and_size.unwrap(), (Kind::Tag, 682));
    }
}
