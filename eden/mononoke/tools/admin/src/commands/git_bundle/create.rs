/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use clap::Args;
use context::CoreContext;
use flate2::write::ZlibDecoder;
use futures::stream;
use futures::Future;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use gix_hash::ObjectId;
use packfile::bundle::BundleWriter;
use packfile::types::PackfileItem;
use walkdir::WalkDir;

const HEAD_REF_PREFIX: &str = "ref: ";
const NON_OBJECTS_DIR: [&str; 2] = ["info", "pack"];
const OBJECTS_DIR: &str = "objects";
const REFS_DIR: &str = "refs";
const HEAD_REF: &str = "HEAD";

#[derive(Args)]
/// Arguments for creating a Git bundle
pub struct CreateBundleArgs {
    /// The location, i.e. file_name + path, where the generated bundle will be stored
    #[clap(long, short = 'o', value_name = "FILE")]
    output_location: PathBuf,
    /// The path to the Git repo where the required objects to be bundled are present
    /// e.g. /repo/path/.git
    #[clap(long, value_name = "FILE")]
    git_repo_path: PathBuf,
}

pub async fn create(_ctx: &CoreContext, create_args: CreateBundleArgs) -> Result<()> {
    // Open the output file for writing
    let output_file = tokio::fs::File::create(create_args.output_location.as_path())
        .await
        .with_context(|| {
            format!(
                "Error in opening/creating output file {}",
                create_args.output_location.display()
            )
        })?;
    // Create a handle for reading the Git directory
    let git_directory =
        std::fs::read_dir(create_args.git_repo_path.as_path()).with_context(|| {
            format!(
                "Error in opening git directory {}",
                create_args.git_repo_path.display()
            )
        })?;
    let mut object_count = 0;
    let mut object_stream = None;
    let mut refs_to_include = HashMap::new();
    let mut head_ref = None;
    // Walk through the Git directory and fetch all objects and refs
    for entry in git_directory {
        let entry = entry.context("Error in opening entry within git directory")?;
        let is_entry_dir = entry.file_type()?.is_dir();
        let entry_name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Non UTF8 entry {:?} found in Git directory",
                    entry.file_name()
                )
            })?
            .to_string();
        match (is_entry_dir, entry_name.as_str()) {
            // Read all the entries within the objects directory and convert into the input stream
            // for writing to bundle
            (true, OBJECTS_DIR) => {
                let object_paths = get_files_in_dir_recursive(entry.path(), |entry| {
                    !NON_OBJECTS_DIR.contains(&entry.file_name().to_str().unwrap_or(""))
                })
                .context("Error in getting files from objects directory")?
                .into_iter()
                .filter(|entry| entry.is_file())
                .collect::<Vec<_>>();
                object_count += object_paths.len();
                object_stream = Some(get_objects_stream(object_paths).await);
            }
            (true, REFS_DIR) => {
                refs_to_include = get_refs(entry.path()).await?;
            }
            (false, HEAD_REF) => {
                head_ref = Some(get_head_ref(entry.path()).await?);
            }
            _ => {}
        };
    }
    // HEAD reference (if exists) points to some other ref instead of directly pointing to a commit.
    // To include the HEAD reference in the bundle, we need to find the commit it points to.
    if let Some(head_ref) = head_ref {
        let pointed_ref = refs_to_include.get(head_ref.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "HEAD ref points to a non-existent ref {}. Known refs: {:?}",
                head_ref,
                refs_to_include
            )
        })?;
        refs_to_include.insert("HEAD".to_string(), *pointed_ref);
    }
    // Create the bundle writer with the header pre-written
    let mut writer = BundleWriter::new_with_header(
        output_file,
        refs_to_include.into_iter().collect(),
        None,
        object_count as u32,
    )
    .await?;
    let object_stream =
        object_stream.ok_or_else(|| anyhow::anyhow!("No objects found to write to bundle"))?;
    // Write the encoded Git object content to the Git bundle
    writer
        .write(object_stream)
        .await
        .context("Error in writing Git objects to bundle")?;
    // Finish writing the bundle
    writer
        .finish()
        .await
        .context("Error in finishing write to bundle")?;
    Ok(())
}

/// Get the ref pointed to by the HEAD reference in the given Git repo
async fn get_head_ref(head_path: PathBuf) -> Result<String> {
    let head_content = tokio::fs::read(head_path.as_path())
        .await
        .with_context(|| format!("Error in opening HEAD file {}", head_path.display()))?;
    let head_str = String::from_utf8(head_content)
        .with_context(|| format!("Non UTF8 content in HEAD file {}", head_path.display()))?;
    head_str
        .trim()
        .strip_prefix(HEAD_REF_PREFIX)
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("Invalid string content {} in HEAD file", head_str))
}

/// Get the list of refs from .git/refs directory along with the ObjectId that they point to.
async fn get_refs(refs_path: PathBuf) -> Result<HashMap<String, ObjectId>> {
    let ref_file_paths = get_files_in_dir_recursive(refs_path.clone(), |_| true)
        .with_context(|| {
            format!(
                "Error in fetching files from .git/refs directory {}",
                refs_path.display()
            )
        })?
        .into_iter()
        .filter(|entry| entry.is_file());
    stream::iter(ref_file_paths.filter(|path| path.is_file()).map(|path| {
        let refs_path = &refs_path;
        async move {
            // Read the contents of the ref file
            let ref_content = tokio::fs::read(path.as_path())
                .await
                .with_context(|| format!("Error in opening ref file {}", path.display()))?;
            // Parse the ref content into an Object ID
            let commit_id = ObjectId::from_hex(&ref_content[..40]).with_context(|| {
                format!(
                    "Error while parsing ref content {:?} at path {:} as Object ID",
                    ref_content.as_slice(),
                    path.display()
                )
            })?;
            let refs_header = path
                .strip_prefix(refs_path.parent().unwrap_or(refs_path.as_path()))
                .with_context(|| {
                    format!("Error in stripping prefix of ref path {}", path.display())
                })?;
            let refs_header = refs_header
                .to_str()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid non-UTF8 path {} in .git/refs",
                        refs_header.display()
                    )
                })?
                .to_string();
            anyhow::Ok((refs_header, commit_id))
        }
    }))
    .buffer_unordered(100)
    .try_collect::<HashMap<String, ObjectId>>()
    .await
}

/// Get the list of objects from the .git/objects directory and return
/// their content as a stream
async fn get_objects_stream(
    object_paths: Vec<PathBuf>,
) -> impl Stream<Item = impl Future<Output = Result<PackfileItem>>> {
    stream::iter(object_paths.into_iter().map(|path| {
        async move {
            // Fetch the Zlib encoded content of the Git object
            let encoded_data = tokio::fs::read(path.as_path())
                .await
                .with_context(|| format!("Error in opening objects file {}", path.display()))?;
            // Decode the content of the Git object
            let mut decoded_data = Vec::new();
            let mut decoder = ZlibDecoder::new(decoded_data);
            decoder.write_all(encoded_data.as_ref())?;
            decoded_data = decoder.finish()?;
            PackfileItem::new_base(Bytes::from(decoded_data))
        }
    }))
}

fn get_files_in_dir_recursive<P>(path: PathBuf, predicate: P) -> Result<Vec<PathBuf>>
where
    P: FnMut(&walkdir::DirEntry) -> bool,
{
    let file_paths = WalkDir::new(path)
        .into_iter()
        .filter_entry(predicate)
        .map(|result| result.map(|entry| entry.into_path()))
        .collect::<std::result::Result<Vec<_>, walkdir::Error>>()?;
    Ok(file_paths)
}
