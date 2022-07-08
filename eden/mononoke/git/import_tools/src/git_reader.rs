/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use git_hash::ObjectId;
use git_object::Kind;
use git_object::Object;
use git_object::ObjectRef;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Mutex;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::process::ChildStdout;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

type ObjectSender = oneshot::Sender<Result<Object>>;

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
            tokio::spawn(read_objects_task(
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
            tokio::spawn(async move {
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
    pub fn get_object(&self, oid: &git_hash::oid) -> impl Future<Output = Result<Object>> {
        let outstanding_requests = self.outstanding_requests.clone();
        let send_request = self.send_request.clone();
        let oid = oid.to_owned();
        async move {
            let permit = send_request.reserve().await?;
            let (sender, recv) = oneshot::channel();
            outstanding_requests
                .lock()
                .expect("lock poisoned")
                .entry(oid.to_owned())
                .or_default()
                .push(sender);

            permit.send(oid);

            recv.await?
        }
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

fn convert_to_object(kind: Kind, bytes: Vec<u8>) -> Result<Object> {
    let object_ref = ObjectRef::from_bytes(kind, &bytes)?;
    Ok(object_ref.into_owned())
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
                        let _ = sender.send(Err(e));
                    }
                    continue;
                }
            };

            // We have an object. Let's try and read it.

            // We need to read size bytes, and then send it on as an object to unblock our listener
            let mut bytes = Vec::new();
            bytes.resize(size, 0u8);
            reader.read_exact(&mut bytes).await?;
            if let Some(sender) = maybe_sender {
                let object = convert_to_object(kind, bytes);
                let _ = sender.send(object);
            }
            // Finally, there's an empty line after the object, but before the next header. Consume it
            reader.read_line(&mut buf).await?;
            buf.clear();
        }
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_kind_str_to_kind() {
        assert!(kind_str_to_kind("forest").is_err());
        assert_eq!(kind_str_to_kind("blob").unwrap(), Kind::Blob);
        assert_eq!(kind_str_to_kind("commit").unwrap(), Kind::Commit);
        assert_eq!(kind_str_to_kind("tag").unwrap(), Kind::Tag);
        assert_eq!(kind_str_to_kind("tree").unwrap(), Kind::Tree);
    }

    #[test]
    fn test_parse_kind_and_size() {
        assert!(parse_kind_and_size("missing").is_err());
        assert_eq!(
            parse_kind_and_size("tree 12345").unwrap(),
            (Kind::Tree, 12345)
        );
    }

    #[test]
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
