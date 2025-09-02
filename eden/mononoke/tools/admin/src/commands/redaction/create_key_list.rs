/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use clap::ArgGroup;
use clap::Args;
use commit_id::parse_commit_id;
use context::CoreContext;
use fsnodes::RootFsnodeId;
use futures::stream::TryStreamExt;
use manifest::Entry;
use manifest::ManifestOps;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_app::args::RepoBlobstoreArgs;
use mononoke_types::BlobstoreKey;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use mononoke_types::typed_hash::RedactionKeyListId;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;

use super::Repo;
use super::list::paths_for_content_keys;

#[derive(Args)]
#[clap(group(ArgGroup::new("files-input=file").args(&["files", "input_file"]).required(true)))]
pub struct RedactionCreateKeyListArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(long, short = 'i')]
    commit_id: String,

    /// Fail if any of the content to be redacted is reachable from this main
    /// bookmark unless --force is set.
    #[clap(long, default_value = "master")]
    main_bookmark: BookmarkKey,

    /// Force content redaction even if content is reachable from the main
    /// bookmark.
    #[clap(long)]
    force: bool,

    /// Name of a file with a list of filenames to redact.
    #[clap(long)]
    input_file: Option<PathBuf>,

    /// Name of a file to write the new key to.
    #[clap(long)]
    output_file: Option<PathBuf>,

    /// Files to redact
    #[clap(value_name = "FILE")]
    files: Vec<String>,
}

#[derive(Args)]
pub struct RedactionCreateKeyListFromIdsArgs {
    #[clap(flatten)]
    repo_blobstore_args: RepoBlobstoreArgs,

    /// Blobstore keys to redact
    #[clap(value_name = "KEY")]
    keys: Vec<String>,

    /// Name of a file to write the new key to.
    #[clap(long)]
    output_file: Option<PathBuf>,
}

#[derive(Args)]
pub struct RedactionFetchKeyListArgs {
    #[clap(flatten)]
    repo_blobstore_args: RepoBlobstoreArgs,

    /// Redaction key list id, as obtained from `create-key-list` or `create-key-list-from-id`
    #[clap(value_name = "KEY LIST ID")]
    key_list_id: RedactionKeyListId,

    /// Name of a file to write the key list to.
    #[clap(long)]
    output_file: Option<PathBuf>,
}

pub async fn fetch_key_list(
    ctx: &CoreContext,
    app: &MononokeApp,
    args: RedactionFetchKeyListArgs,
) -> Result<()> {
    let redaction_blobstore = app.redaction_config_blobstore().await?;
    let key_list = redaction::fetch_key_list(ctx, &redaction_blobstore, args.key_list_id).await?;
    if let Some(output_file) = args.output_file.as_deref() {
        let mut output = File::create(output_file).with_context(|| {
            format!(
                "Failed to open output file '{}'",
                output_file.to_string_lossy()
            )
        })?;
        for key in key_list.keys {
            output
                .write(format!("{}\n", key).as_bytes())
                .with_context(|| {
                    format!(
                        "Failed to write to output file '{}'",
                        output_file.to_string_lossy()
                    )
                })?;
        }
    }
    Ok(())
}

async fn create_key_list(
    ctx: &CoreContext,
    app: &MononokeApp,
    keys: Vec<String>,
    output_file: Option<&Path>,
) -> Result<()> {
    let redaction_blobstore = app.redaction_config_blobstore().await?;
    let key_list_id = redaction::create_key_list(ctx, &redaction_blobstore, keys).await?;
    if let Some(output_file) = output_file {
        let mut output = File::create(output_file).with_context(|| {
            format!(
                "Failed to open output file '{}'",
                output_file.to_string_lossy()
            )
        })?;
        output
            .write_all(key_list_id.to_string().as_bytes())
            .with_context(|| {
                format!(
                    "Failed to write to output file '{}'",
                    output_file.to_string_lossy()
                )
            })?;
    }
    Ok(())
}

/// Returns the content keys for the given paths.
async fn content_keys_for_paths(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    paths: Vec<NonRootMPath>,
) -> Result<HashSet<String>> {
    let root_fsnode_id = repo
        .repo_derived_data()
        .derive::<RootFsnodeId>(ctx, cs_id)
        .await?;
    let path_content_keys = root_fsnode_id
        .fsnode_id()
        .find_entries(ctx.clone(), repo.repo_blobstore_arc(), paths.clone())
        .try_filter_map(|(path, entry)| async move {
            match (path.into_optional_non_root_path(), entry) {
                (Some(path), Entry::Leaf(fsnode_file)) => {
                    Ok(Some((path, fsnode_file.content_id().blobstore_key())))
                }
                _ => Ok(None),
            }
        })
        .try_collect::<HashMap<_, _>>()
        .await?;

    let mut missing_paths = 0;
    for path in paths.iter() {
        if !path_content_keys.contains_key(path) {
            eprintln!("Missing file: {}", path);
            missing_paths += 1;
        }
    }
    if missing_paths > 0 {
        bail!("Failed to find {} files in this commit", missing_paths);
    }

    Ok(path_content_keys.into_values().collect())
}

pub async fn create_key_list_from_commit_files(
    ctx: &CoreContext,
    app: &MononokeApp,
    create_args: RedactionCreateKeyListArgs,
) -> Result<()> {
    let mut files = create_args
        .files
        .iter()
        .map(NonRootMPath::new)
        .collect::<Result<Vec<_>>>()?;
    if let Some(input_file) = create_args.input_file {
        let input_file =
            BufReader::new(File::open(input_file).context("Failed to open input file")?);
        for line in input_file.lines() {
            files.push(NonRootMPath::new(line?)?);
        }
    }
    if files.is_empty() {
        bail!("No files to redact");
    }
    let repo: Repo = app
        .open_repo(&create_args.repo_args)
        .await
        .context("Failed to open repo")?;

    let cs_id = parse_commit_id(ctx, &repo, &create_args.commit_id).await?;

    let keys = content_keys_for_paths(ctx, &repo, cs_id, files).await?;

    println!(
        "Checking redacted content doesn't exist in '{}' bookmark",
        create_args.main_bookmark
    );
    let main_cs_id = repo
        .bookmarks()
        .get(
            ctx.clone(),
            &create_args.main_bookmark,
            bookmarks::Freshness::MostRecent,
        )
        .await?
        .ok_or_else(|| {
            anyhow!(
                "Main bookmark '{}' does not exist",
                create_args.main_bookmark
            )
        })?;
    let main_redacted = paths_for_content_keys(ctx, &repo, main_cs_id, &keys).await?;

    if main_redacted.is_empty() {
        println!(
            "No files would be redacted in the main bookmark ({})",
            create_args.main_bookmark
        );
    } else {
        for (path, content_id) in main_redacted.iter() {
            println!(
                "Redacted content in main bookmark: {} {}",
                path,
                content_id.blobstore_key(),
            );
        }
        if create_args.force {
            println!(
                "Creating key list despite {} files being redacted in the main bookmark ({}) (--force)",
                main_redacted.len(),
                create_args.main_bookmark
            );
        } else {
            bail!(
                "Refusing to create key list because {} files would be redacted in the main bookmark ({})",
                main_redacted.len(),
                create_args.main_bookmark
            );
        }
    }

    create_key_list(
        ctx,
        app,
        keys.into_iter().collect(),
        create_args.output_file.as_deref(),
    )
    .await
}

pub async fn create_key_list_from_blobstore_keys(
    ctx: &CoreContext,
    app: &MononokeApp,
    create_args: RedactionCreateKeyListFromIdsArgs,
) -> Result<()> {
    create_key_list(
        ctx,
        app,
        create_args.keys,
        create_args.output_file.as_deref(),
    )
    .await
}
