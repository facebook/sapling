/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use changesets_creation::save_changesets;
use clap::Parser;
use mercurial_derived_data::MappedHgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use serde_derive::Deserialize;
use sorted_vector_map::SortedVectorMap;

/// Create commits from a JSON-encoded bonsai changeset
///
/// The bonsai changeset is intentionally not checked for correctness, as this
/// may be used in tests to test handling of malformed bonsai changesets.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    /// Path to a file containing a JSON-encoded bonsai changeset
    #[clap(parse(from_os_str))]
    bonsai_file: PathBuf,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let mut content = String::new();
    File::open(&args.bonsai_file)
        .with_context(|| {
            format!(
                "Failed to open bonsai changeset file '{}'",
                args.bonsai_file.to_string_lossy()
            )
        })?
        .read_to_string(&mut content)
        .context("Failed to read bonsai changeset file")?;

    let bcs: BonsaiChangeset = serde_json::from_str::<DeserializableBonsaiChangeset>(&content)
        .context("Failed to parse bonsai changeset data")?
        .into_bonsai()?
        .freeze()?;

    let repo: BlobRepo = app
        .open_repo(&args.repo_args)
        .await
        .context("Failed to open repo")?;

    for (_, change) in bcs.simplified_file_changes() {
        match change {
            Some(tc) => {
                if filestore::get_metadata(repo.repo_blobstore(), &ctx, &tc.content_id().into())
                    .await?
                    .is_none()
                {
                    return Err(anyhow!(
                        "file content {} is not found in the filestore",
                        &tc.content_id()
                    ));
                }
            }
            None => {}
        }
    }
    let bcs_id = bcs.get_changeset_id();
    save_changesets(&ctx, &repo, vec![bcs])
        .await
        .context("Failed to save changeset")?;
    let hg_cs = repo
        .repo_derived_data()
        .derive::<MappedHgChangesetId>(&ctx, bcs_id)
        .await
        .context("Failed to derive Mercurial changeset")?
        .hg_changeset_id();
    println!(
        "Created bonsai changeset {} for Hg changeset {}",
        bcs_id, hg_cs
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct DeserializableBonsaiChangeset {
    pub parents: Vec<ChangesetId>,
    pub author: String,
    pub author_date: DateTime,
    pub committer: Option<String>,
    pub committer_date: Option<DateTime>,
    pub message: String,
    pub extra: BTreeMap<String, Vec<u8>>,
    pub file_changes: BTreeMap<String, FileChange>,
}

impl DeserializableBonsaiChangeset {
    pub fn into_bonsai(self) -> Result<BonsaiChangesetMut, Error> {
        let files = self
            .file_changes
            .into_iter()
            .map::<Result<_, Error>, _>(|(path, changes)| {
                Ok((MPath::new(path.as_bytes())?, changes))
            })
            .collect::<Result<SortedVectorMap<_, _>, _>>()?;
        Ok(BonsaiChangesetMut {
            parents: self.parents,
            author: self.author,
            author_date: self.author_date,
            committer: self.committer,
            committer_date: self.committer_date,
            message: self.message,
            extra: self.extra.into(),
            file_changes: files,
            is_snapshot: false,
        })
    }
}
