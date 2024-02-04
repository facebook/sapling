/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use bonsai_git_mapping::BonsaiGitMapping;
use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
use clap::Parser;
use commit_id::parse_commit_id;
use commit_id::print_commit_id;
use commit_id::IdentityScheme;
use git_types::MappedGitCommitId;
use mercurial_derivation::MappedHgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use repo_derived_data::RepoDerivedData;
use repo_derived_data::RepoDerivedDataRef;

/// Convert commit identity between identity schemes
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Identity scheme to convert from
    #[clap(long, short = 'f', value_enum, value_name = "SCHEME")]
    from: Option<IdentityScheme>,

    /// Identity scheme to convert to
    #[clap(
        long,
        short = 't',
        value_enum,
        value_name = "SCHEME",
        required = true,
        use_value_delimiter = true
    )]
    to: Vec<IdentityScheme>,

    /// Source commit id
    id: String,

    /// Derive the target commit type if necessary
    #[clap(long)]
    derive: bool,
}

#[facet::container]
pub struct Repo {
    #[facet]
    bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    bonsai_git_mapping: dyn BonsaiGitMapping,

    #[facet]
    bonsai_globalrev_mapping: dyn BonsaiGlobalrevMapping,

    #[facet]
    bonsai_svnrev_mapping: dyn BonsaiSvnrevMapping,

    #[facet]
    repo_derived_data: RepoDerivedData,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let repo: Repo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    let cs_id = match args.from {
        Some(scheme) => scheme.parse_commit_id(&ctx, &repo, &args.id).await?,
        None => parse_commit_id(&ctx, &repo, &args.id).await?,
    };

    if args.derive {
        for to in args.to.iter() {
            match to {
                IdentityScheme::Hg => {
                    repo.repo_derived_data()
                        .derive::<MappedHgChangesetId>(&ctx, cs_id)
                        .await
                        .context("Failed to derive Mercurial changeset")?;
                }
                IdentityScheme::Git => {
                    repo.repo_derived_data()
                        .derive::<MappedGitCommitId>(&ctx, cs_id)
                        .await
                        .context("Failed to derive Git commit")?;
                }
                _ => {}
            }
        }
    }

    print_commit_id(&ctx, &repo, &args.to, cs_id).await?;

    Ok(())
}
