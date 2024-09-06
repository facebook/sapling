/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

mod repo;

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use blobstore::Loadable;
use bonsai_hg_mapping::ArcBonsaiHgMapping;
use bonsai_hg_mapping::MemWritesBonsaiHgMapping;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use cacheblob::dummy::DummyLease;
use cacheblob::LeaseOps;
use cacheblob::MemWritesBlobstore;
use clap::Parser;
use clap::Subcommand;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use git_symbolic_refs::GitSymbolicRefsRef;
use import_tools::bookmark::BookmarkOperationErrorReporting;
use import_tools::create_changeset_for_annotated_tag;
use import_tools::import_tree_as_single_bonsai_changeset;
use import_tools::set_bookmark;
use import_tools::upload_git_tag;
use import_tools::BackfillDerivation;
use import_tools::BookmarkOperation;
use import_tools::GitImportLfs;
use import_tools::GitRepoReader;
use import_tools::GitimportPreferences;
use import_tools::GitimportTarget;
use import_tools::ReuploadCommits;
use linked_hash_map::LinkedHashMap;
use mercurial_derivation::get_manifest_from_bonsai;
use mercurial_derivation::DeriveHgChangeset;
use metaconfig_types::RepoConfigRef;
use mononoke_api::BookmarkFreshness;
use mononoke_api::BookmarkKey;
use mononoke_api::RepoContext;
use mononoke_app::args::RepoArgs;
use mononoke_app::args::TLSArgs;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use repo_authorization::AuthorizationContext;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use slog::warn;

use crate::repo::Repo;

pub const HEAD_SYMREF: &str = "HEAD";
const LFS_SIMULTANEOUS_CONNECTION_LIMIT: usize = 20;

// Refactor this a bit. Use a thread pool for git operations. Pass that wherever we use store repo.
// Transform the walk into a stream of commit + file changes.

async fn derive_hg(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreArc + RepoDerivedDataRef + RepoIdentityRef + Send + Sync),
    import_map: impl Iterator<Item = (&gix_hash::ObjectId, &ChangesetId)>,
) -> Result<(), Error> {
    let mut hg_manifests = HashMap::new();

    for (id, bcs_id) in import_map {
        let bcs = bcs_id.load(ctx, repo.repo_blobstore()).await?;
        let parent_manifests = future::try_join_all(bcs.parents().map({
            let hg_manifests = &hg_manifests;
            move |p| async move {
                let manifest = if let Some(manifest) = hg_manifests.get(&p) {
                    *manifest
                } else {
                    repo.derive_hg_changeset(ctx, p)
                        .await?
                        .load(ctx, repo.repo_blobstore())
                        .await?
                        .manifestid()
                };
                Result::<_, Error>::Ok(manifest)
            }
        }))
        .await?;

        let manifest = get_manifest_from_bonsai(
            ctx.clone(),
            repo.repo_blobstore_arc(),
            bcs.clone(),
            parent_manifests,
        )
        .await?;

        hg_manifests.insert(*bcs_id, manifest);

        info!(ctx.logger(), "Hg: {:?}: {:?}", id, manifest);
    }

    Ok(())
}

/// Mononoke Git Importer
#[derive(Parser)]
struct GitimportArgs {
    #[clap(long)]
    derive_hg: bool,
    /// This is used to suppress the printing of the potentially really long git Reference -> BonzaiID mapping.
    #[clap(long)]
    suppress_ref_mapping: bool,
    /// **Dangerous** Generate bookmarks for all git refs (tags and branches)
    /// Make sure not to use this on a mononoke repo in production or you will overwhelm any
    /// service doing backfilling on public changesets!
    /// Use at your own risk!
    #[clap(long)]
    generate_bookmarks: bool,
    /// If set, will skip recording the HEAD symref in Mononoke for the given repo
    #[clap(long)]
    skip_head_symref: bool,
    /// When set, the gitimport tool would bypass the read-only check while creating and moving bookmarks.
    #[clap(long)]
    bypass_readonly: bool,
    /// The concurrency to be used while importing commits in Mononoke
    #[clap(long, default_value_t = 20)]
    concurrency: usize,
    /// Set the path to the git binary - preset to git.real
    #[clap(long)]
    git_command_path: Option<String>,
    /// Path to a git repository to import
    git_repository_path: String,
    /// Reupload git commits, even if they already exist in Mononoke
    #[clap(long)]
    reupload_commits: bool,
    #[clap(subcommand)]
    subcommand: GitimportSubcommand,
    #[clap(flatten)]
    repo_args: RepoArgs,
    /// Discard any git submodule during import.
    /// **WARNING**: This will make the repo import lossy: round trip between Mononoke and git won't be
    /// possible anymore.
    /// In particular, this is not suitable as a precursor step to setting up live sync with
    /// Mononoke.
    /// Only use if you are sure that's what you want.
    #[clap(long)]
    discard_submodules: bool,
    /// Don't backfill derived data during this import.
    /// This is dangerous if used in conjunction with generate_bookmarks as public
    /// commits which are not derived may create high load for the derived data service
    #[clap(long)]
    bypass_derived_data_backfilling: bool,
    /// The refs to exclude while importing the repo. Can be used to skip cross-synced refs to avoid
    /// race condition with live gitimport
    #[clap(long, use_value_delimiter = true, value_delimiter = ',')]
    exclude_refs: Vec<String>,
    /// The refs to be included while importing the repo. When provided, gitimport will only import the
    /// explicitly specified refs
    #[clap(long, use_value_delimiter = true, value_delimiter = ',')]
    include_refs: Vec<String>,
    /// Lfs server url to use to fetch lfs files from
    #[clap(long)]
    lfs_server: Option<String>,
    /// TLS parameters for this service used for outbound LFS connections
    #[clap(flatten)]
    tls_args: Option<TLSArgs>,
    /// If LFS file can't be obtained from LFS server, don't fail the import
    /// but import the pointer as-is.
    #[clap(long)]
    allow_dangling_lfs_pointers: bool,
    /// How many times to retry fetching LFS files from the server
    /// before deciding that the file is missing.
    #[clap(long, default_value_t = 5)]
    lfs_import_max_attempts: u32,
}

#[derive(Subcommand)]
enum GitimportSubcommand {
    /// Import all of the commits in this repo
    FullRepo,
    /// Import all commits between <GIT_FROM> and <GIT_TO>
    GitRange {
        git_from: String,
        git_to: String,
    },
    /// Import <GIT_COMMIT> and all its history that's not yet been imported.
    /// Makes a pass over the repo on construction to find missing history
    MissingForCommit {
        git_commit: String,
    },
    ImportTreeAsSingleBonsaiChangeset {
        git_commit: String,
    },
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .build::<GitimportArgs>()?;

    app.run_with_monitoring_and_logging(async_main, "gitimport", AliveService)
}

async fn async_main(app: MononokeApp) -> Result<(), Error> {
    let logger = app.logger();
    let ctx = CoreContext::new_with_logger_and_client_info(
        app.fb,
        logger.clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::GitImport),
    );
    let args: GitimportArgs = app.args()?;
    let path = Path::new(&args.git_repository_path);

    let reupload = if args.reupload_commits {
        ReuploadCommits::Always
    } else {
        ReuploadCommits::Never
    };

    let repo: Repo = app.open_repo(&args.repo_args).await?;
    info!(
        logger,
        "using repo \"{}\" repoid {:?}",
        repo.repo_identity().name(),
        repo.repo_identity().id(),
    );

    let dry_run = app.readonly_storage().0;
    let repo = if dry_run {
        repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            Arc::new(MemWritesBlobstore::new(blobstore))
        })
        .dangerous_override(|bonsai_hg_mapping| -> ArcBonsaiHgMapping {
            Arc::new(MemWritesBonsaiHgMapping::new(bonsai_hg_mapping))
        })
        .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
    } else {
        repo
    };
    let backfill_derivation = if args.bypass_derived_data_backfilling {
        if args.generate_bookmarks {
            warn!(
                logger,
                "Warning: gitimport was called bypassing derived data backfilling while generating bookmarks.\nIt is your responsibility to ensure that all derived data types are backfilled before this repository is expose to prod to avoid the risk of overloading the derived data service."
            );
        }
        BackfillDerivation::No
    } else if args.discard_submodules {
        let configured_types = &repo.repo_derived_data().active_config().types;
        BackfillDerivation::OnlySpecificTypes(
            configured_types
                .iter()
                .filter(|ty| match ty {
                    // If we discard submodules, we can't derive the git data types since they are inconsistent
                    DerivableType::GitCommits
                    | DerivableType::GitDeltaManifestsV2
                    | DerivableType::GitTrees => false,
                    _ => true,
                })
                .cloned()
                .collect(),
        )
    } else {
        BackfillDerivation::AllConfiguredTypes
    };
    let lfs = match repo.repo_config().git_configs.git_lfs_interpret_pointers {
        true => GitImportLfs::new(
            args.lfs_server.ok_or_else(|| {
                anyhow!("LFS server url is required when LFS is enabled in the repo config")
            })?,
            args.allow_dangling_lfs_pointers,
            args.lfs_import_max_attempts,
            Some(LFS_SIMULTANEOUS_CONNECTION_LIMIT),
            args.tls_args,
        )?,
        false => GitImportLfs::new_disabled(),
    };

    let mut prefs = GitimportPreferences {
        concurrency: args.concurrency,
        submodules: !args.discard_submodules,
        backfill_derivation,
        lfs,
        ..Default::default()
    };
    // if we are readonly, then we'll set up some overrides to still be able to do meaningful
    // things below.
    prefs.dry_run = dry_run;

    if let Some(path) = args.git_command_path {
        prefs.git_command_path = PathBuf::from(path);
    }

    let uploader = Arc::new(import_direct::DirectUploader::new(repo.clone(), reupload));

    let target = match args.subcommand {
        GitimportSubcommand::FullRepo {} => GitimportTarget::full(),
        GitimportSubcommand::GitRange { git_from, git_to } => {
            let from = git_from.parse()?;
            let to = git_to.parse()?;
            import_direct::range(from, to, &ctx, &repo).await?
        }
        GitimportSubcommand::MissingForCommit { git_commit } => {
            let commit = git_commit.parse()?;
            import_direct::missing_for_commit(commit, &ctx, &repo, &prefs.git_command_path, path)
                .await?
        }
        GitimportSubcommand::ImportTreeAsSingleBonsaiChangeset { git_commit } => {
            let commit = git_commit.parse()?;
            let bcs_id = import_tree_as_single_bonsai_changeset(
                &ctx,
                path,
                uploader.clone(),
                commit,
                &prefs,
            )
            .await?;
            info!(ctx.logger(), "imported as {}", bcs_id);
            if args.derive_hg {
                derive_hg(&ctx, &repo, [(&commit, &bcs_id)].into_iter()).await?;
            }
            return Ok(());
        }
    };

    let gitimport_result: LinkedHashMap<_, _> =
        import_tools::gitimport(&ctx, path, uploader.clone(), &target, &prefs)
            .await
            .context("gitimport failed")?;
    if args.derive_hg {
        derive_hg(&ctx, &repo, gitimport_result.iter())
            .await
            .context("derive_hg failed")?;
    }
    if !args.skip_head_symref {
        let symref_entry = import_tools::read_symref(HEAD_SYMREF, path, &prefs)
            .await
            .context("read_symrefs failed")?;
        repo.git_symbolic_refs()
            .add_or_update_entries(vec![symref_entry])
            .await
            .context("failed to add symbolic ref entries")?;
    }
    if !args.suppress_ref_mapping || args.generate_bookmarks {
        let refs = import_tools::read_git_refs(path, &prefs)
            .await
            .context("read_git_refs failed")?;
        let mapping = refs
            .into_iter()
            .map(|(git_ref, commit)| {
                Ok((
                    git_ref.maybe_tag_id,
                    String::from_utf8(git_ref.name).map_err(|err| {
                        anyhow::anyhow!(
                            "Failed to parse git ref name {:?} due to invalid UTF-8 encoding, Cause: {}",
                            err.as_bytes(),
                            err.utf8_error()
                        )
                    })?,
                    gitimport_result.get(&commit),
                ))
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        if !args.suppress_ref_mapping {
            for (_, name, changeset) in &mapping {
                info!(ctx.logger(), "Ref: {:?}: {:?}", name, changeset);
            }
        }
        if args.generate_bookmarks {
            let authz = AuthorizationContext::new_bypass_access_control();
            let repo_context: RepoContext<mononoke_api::Repo> = app
                .open_managed_repo_arg(&args.repo_args)
                .await
                .context("failed to create mononoke app")?
                .make_mononoke_api()?
                .repo_by_id(ctx.clone(), repo.repo_identity().id())
                .await
                .with_context(|| format!("failed to access repo: {}", repo.repo_identity().id()))?
                .expect("repo exists")
                .with_authorization_context(authz)
                .build()
                .await
                .context("failed to build RepoContext")?;
            let existing_tags = repo
                .bonsai_tag_mapping()
                .get_all_entries()
                .await
                .context("Failed to fetch bonsai tag mapping")?
                .into_iter()
                .map(|entry| (entry.tag_name, entry.tag_hash))
                .collect::<HashMap<_, _>>();
            let reader = Arc::new(GitRepoReader::new(&prefs.git_command_path, path).await?);
            for (maybe_tag_id, name, changeset) in
                mapping
                    .iter()
                    .filter_map(|(maybe_tag_id, name, changeset)| {
                        // Exclude the ref if its specified in the exclude-list OR if its not explicitly specified in the include-list (if exists)
                        let exclude_ref = args.exclude_refs.contains(name)
                            || !(args.include_refs.is_empty() || args.include_refs.contains(name));
                        if exclude_ref {
                            None
                        } else {
                            changeset.map(|cs| (maybe_tag_id, name, cs))
                        }
                    })
            {
                let final_changeset = changeset.clone();
                let name = name
                    .strip_prefix("refs/")
                    .context("Ref does not start with refs/")?
                    .to_string();
                if name.as_str() == "heads/HEAD" {
                    // Skip the HEAD revision: it shouldn't be imported as a bookmark in mononoke
                    continue;
                }
                if let Some(tag_id) = maybe_tag_id {
                    let new_or_updated_tag = existing_tags.get(&name).map_or(true, |tag_hash| {
                        if let Ok(new_hash) = GitSha1::from_object_id(tag_id) {
                            *tag_hash != new_hash
                        } else {
                            false
                        }
                    });
                    // Only upload the tag if it's new or has changed.
                    if new_or_updated_tag || reupload.reupload_commit() {
                        // The ref getting imported is a tag, so store the raw git Tag object.
                        upload_git_tag(&ctx, uploader.clone(), reader.clone(), tag_id).await?;
                        // Create the changeset corresponding to the commit pointed to by the tag.
                        create_changeset_for_annotated_tag(
                            &ctx,
                            uploader.clone(),
                            reader.clone(),
                            tag_id,
                            Some(name.clone()),
                            changeset,
                        )
                        .await?;
                    }
                }
                let bookmark_key = BookmarkKey::new(&name)?;

                let pushvars = if args.bypass_readonly {
                    Some(HashMap::from_iter([(
                        "BYPASS_READONLY".to_string(),
                        bytes::Bytes::from("true"),
                    )]))
                } else {
                    None
                };
                let old_changeset = repo_context
                    .resolve_bookmark(&bookmark_key, BookmarkFreshness::MostRecent)
                    .await
                    .with_context(|| format!("failed to resolve bookmark {name}"))?
                    .map(|context| context.id());
                let allow_non_fast_forward = true;
                let operation =
                    BookmarkOperation::new(bookmark_key, old_changeset, Some(final_changeset))?;
                set_bookmark(
                    &ctx,
                    &repo_context,
                    &operation,
                    pushvars.as_ref(),
                    allow_non_fast_forward,
                    BookmarkOperationErrorReporting::WithContext,
                )
                .await?;
            }
        };
    }
    Ok(())
}
