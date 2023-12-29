/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use clap::Args;
use context::CoreContext;
use flate2::write::ZlibDecoder;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use gix_hash::ObjectId;
use mononoke_api::ChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use packfile::bundle::BundleWriter;
use packfile::pack::DeltaForm;
use packfile::types::PackfileItem;
use protocol::generator::generate_pack_item_stream;
use protocol::types::DeltaInclusion;
use protocol::types::PackItemStreamRequest;
use protocol::types::PackfileItemInclusion;
use protocol::types::RequestedRefs;
use protocol::types::RequestedSymrefs;
use protocol::types::TagInclusion;
use walkdir::WalkDir;

use super::Repo;

const HEAD_REF_PREFIX: &str = "ref: ";
const NON_OBJECTS_DIR: [&str; 2] = ["info", "pack"];
const OBJECTS_DIR: &str = "objects";
const REFS_DIR: &str = "refs";
const HEAD_REF: &str = "HEAD";

/// Parse a single key-value pair
fn parse_key_val(s: &str) -> Result<(String, ChangesetId)> {
    let pos = s
        .find('=')
        .ok_or_else(|| anyhow::anyhow!("invalid KEY=value: no `=` found in `{s}`"))?;
    Ok((s[..pos].to_string(), ChangesetId::from_str(&s[pos + 1..])?))
}

/// Args for creating a Git bundle from a Mononoke repo
#[derive(Args)]
pub struct FromRepoArgs {
    /// The Mononoke repo for which the Git bundle should be created
    #[clap(flatten)]
    repo: RepoArgs,
    /// The set of references that should be included in the bundle. The value of these refs
    /// (i.e. the commits that the ref point to) would be as seen by the server. If empty,
    /// (along with included_refs_with_value) all the references will be included in the bundle
    /// with the value as seen by the server
    #[clap(
        long,
        value_delimiter = ',',
        conflicts_with = "included_refs_with_value"
    )]
    included_refs: Vec<String>,
    /// The set of references that should be included in the bundle along with the provided values
    /// If empty, (along with included_refs) all the references will be included in the bundle with
    /// the value as seen by the server
    #[clap(long, value_delimiter = ',', value_parser = parse_key_val, conflicts_with = "included_refs")]
    included_refs_with_value: Vec<(String, ChangesetId)>,
    /// The set of commits/changesets that are already present and can be used as
    /// prerequisites for the bundle. If empty, then the bundle will record the entire
    /// history of the repo for the included_refs
    #[clap(long, value_delimiter = ',')]
    have_heads: Vec<ChangesetId>,
    /// The location, i.e. file_name + path, where the generated bundle will be stored
    #[clap(long, short = 'o', value_name = "FILE")]
    output_location: PathBuf,
    /// Flag controlling whether the generated bundle can contains deltas or just full object
    #[clap(long, conflicts_with = "exclude_ref_deltas")]
    exclude_deltas: bool,
    /// Flag controlling whether the generated bundle can contains ref deltas or just offset deltas
    /// (for the delta objects)
    #[clap(long, conflicts_with = "exclude_deltas")]
    exclude_ref_deltas: bool,
    /// The concurrency with which the stream objects will be prefetched while writing to the bundle
    #[clap(long, default_value_t = 1000)]
    concurrency: usize,
    /// Should the packfile items for base objects be generated on demand or fetched from store
    #[clap(long, default_value_t, value_enum)]
    packfile_item_inclusion: PackfileItemInclusion,
}

/// Args for creating a Git bundle from an on-disk Git repo
#[derive(Args)]
pub struct FromPathArgs {
    /// The path to the Git repo where the required objects to be bundled are present
    /// e.g. /repo/path/.git
    #[clap(long, value_name = "FILE")]
    git_repo_path: PathBuf,
    /// The location, i.e. file_name + path, where the generated bundle will be stored
    #[clap(long, short = 'o', value_name = "FILE")]
    output_location: PathBuf,
}

pub async fn create_from_path(create_args: FromPathArgs) -> Result<()> {
    // Open the output file for writing
    let output_file = tokio::fs::File::create(create_args.output_location.as_path())
        .await
        .with_context(|| {
            format!(
                "Error in opening/creating output file {}",
                create_args.output_location.display()
            )
        })?;
    create_from_on_disk_repo(create_args.git_repo_path, output_file).await
}

pub async fn create_from_mononoke_repo(
    ctx: &CoreContext,
    app: &MononokeApp,
    create_args: FromRepoArgs,
) -> Result<()> {
    // Open the output file for writing
    let output_file = tokio::fs::File::create(create_args.output_location.as_path())
        .await
        .with_context(|| {
            format!(
                "Error in opening/creating output file {}",
                create_args.output_location.display()
            )
        })?;
    let repo: Repo = app
        .open_repo(&create_args.repo)
        .await
        .context("Failed to open repo")?;
    let delta_inclusion = if create_args.exclude_deltas {
        DeltaInclusion::Exclude
    } else {
        let form = if create_args.exclude_ref_deltas {
            DeltaForm::OnlyOffset
        } else {
            DeltaForm::RefAndOffset
        };
        DeltaInclusion::Include {
            form,
            inclusion_threshold: 0.90,
        }
    };
    // If references are specified without values, just take the ref names
    let requested_refs = if !create_args.included_refs.is_empty() {
        RequestedRefs::Included(create_args.included_refs.into_iter().collect())
    } else if !create_args.included_refs_with_value.is_empty() {
        // Otherwise if refs are provided with values, take the ref name and its value
        RequestedRefs::IncludedWithValue(create_args.included_refs_with_value.into_iter().collect())
    } else {
        // Otherwise include all the refs known by the server
        RequestedRefs::all()
    };
    let request = PackItemStreamRequest::new(
        RequestedSymrefs::IncludeHead,
        requested_refs,
        create_args.have_heads,
        delta_inclusion,
        TagInclusion::AsIs,
        create_args.packfile_item_inclusion,
    );
    let response = generate_pack_item_stream(ctx, &repo, request)
        .await
        .context("Error in generating pack item stream")?;
    // Since this is a full clone
    let prereqs: Option<Vec<ObjectId>> = None;
    // Create the bundle writer with the header pre-written
    let mut writer = BundleWriter::new_with_header(
        output_file,
        response.included_refs.into_iter().collect(),
        prereqs,
        response.num_items as u32,
        create_args.concurrency,
        DeltaForm::RefAndOffset, // Ref deltas are supported by Git when cloning from a bundle
    )
    .await?;

    writer
        .write(response.items)
        .await
        .context("Error in writing packfile items to bundle")?;
    // Finish writing the bundle
    writer
        .finish()
        .await
        .context("Error in finishing write to bundle")?;
    Ok(())
}

async fn create_from_on_disk_repo(path: PathBuf, output_file: tokio::fs::File) -> Result<()> {
    // Create a handle for reading the Git directory
    let git_directory = std::fs::read_dir(path.as_path())
        .with_context(|| format!("Error in opening git directory {}", path.display()))?;
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
        1000,
        DeltaForm::RefAndOffset,
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
) -> impl Stream<Item = Result<PackfileItem>> {
    stream::iter(object_paths.into_iter().map(anyhow::Ok)).and_then(move |path| {
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
    })
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
