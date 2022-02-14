/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Context, Result};
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_svnrev_mapping::RepoBonsaiSvnrevMappingRef;
use clap::{ArgEnum, Parser};
use mercurial_types::HgChangesetId;
use mononoke_app::args::RepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::hash::GitSha1;
use mononoke_types::{ChangesetId, Globalrev, Svnrev};
use repo_identity::RepoIdentityRef;

use crate::repo::AdminRepo;

#[derive(Copy, Clone, Eq, PartialEq, ArgEnum)]
pub enum IdentityScheme {
    /// Mononoke bonsai hash
    Bonsai,

    /// Mercurial hash
    Hg,

    /// Git SHA-1 hash
    Git,

    /// Globalrev
    Globalrev,

    /// Subversion revision (legacy)
    Svnrev,
}

/// Convert commit identity between identity schemes
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Identity scheme to convert from
    #[clap(long, short = 'f', arg_enum)]
    from: IdentityScheme,

    /// Identity scheme to
    #[clap(long, short = 't', arg_enum)]
    to: IdentityScheme,

    /// Source commit id
    id: String,
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_context();

    let repo: AdminRepo = app
        .open_repo(&args.repo)
        .await
        .context("Failed to open repo")?;

    let cs_id = match args.from {
        IdentityScheme::Bonsai => args
            .id
            .parse::<ChangesetId>()
            .context("Invalid bonsai changeset id")?,
        IdentityScheme::Hg => {
            let hg_cs_id = args
                .id
                .parse::<HgChangesetId>()
                .context("Invalid hg changeset id")?;
            repo.bonsai_hg_mapping()
                .get_bonsai_from_hg(&ctx, hg_cs_id)
                .await?
                .ok_or_else(|| anyhow!("hg-bonsai mapping not found for {}", hg_cs_id))?
        }
        IdentityScheme::Git => {
            let git_id = args
                .id
                .parse::<GitSha1>()
                .context("Invalid git changeset id")?;
            repo.bonsai_git_mapping()
                .get_bonsai_from_git_sha1(&ctx, git_id)
                .await?
                .ok_or_else(|| anyhow!("git-bonsai mapping not found for {}", git_id))?
        }
        IdentityScheme::Globalrev => {
            let globalrev = args.id.parse::<Globalrev>().context("Invalid globalrev")?;
            repo.bonsai_globalrev_mapping()
                .get_bonsai_from_globalrev(&ctx, repo.repo_identity().id(), globalrev)
                .await?
                .ok_or_else(|| anyhow!("globalrev-bonsai mapping not found for {}", globalrev))?
        }
        IdentityScheme::Svnrev => {
            let svnrev = args.id.parse::<Svnrev>().context("Invalid svnrev")?;
            repo.repo_bonsai_svnrev_mapping()
                .get_bonsai_from_svnrev(&ctx, svnrev)
                .await?
                .ok_or_else(|| anyhow!("svnrev-bonsai mapping not found for {}", svnrev))?
        }
    };

    match args.to {
        IdentityScheme::Bonsai => {
            println!("{}", cs_id);
        }
        IdentityScheme::Hg => {
            let hg_cs_id = repo
                .bonsai_hg_mapping()
                .get_hg_from_bonsai(&ctx, cs_id)
                .await?
                .ok_or_else(|| anyhow!("bonsai-hg mapping not found for {}", cs_id))?;
            println!("{}", hg_cs_id);
        }
        IdentityScheme::Git => {
            let git_id = repo
                .bonsai_git_mapping()
                .get_git_sha1_from_bonsai(&ctx, cs_id)
                .await?
                .ok_or_else(|| anyhow!("bonsai-git mapping not found for {}", cs_id))?;
            println!("{}", git_id);
        }
        IdentityScheme::Globalrev => {
            let globalrev = repo
                .bonsai_globalrev_mapping()
                .get_globalrev_from_bonsai(&ctx, repo.repo_identity().id(), cs_id)
                .await?
                .ok_or_else(|| anyhow!("bonsai-globalrev mapping not found for {}", cs_id))?;
            println!("{}", globalrev);
        }
        IdentityScheme::Svnrev => {
            let svnrev = repo
                .repo_bonsai_svnrev_mapping()
                .get_svnrev_from_bonsai(&ctx, cs_id)
                .await?
                .ok_or_else(|| anyhow!("bonsai-svnrev mapping not found for {}", cs_id))?;
            println!("{}", svnrev);
        }
    }

    Ok(())
}
