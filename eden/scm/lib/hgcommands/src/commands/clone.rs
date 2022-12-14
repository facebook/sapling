/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use async_runtime::block_unless_interrupted as block_on;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::errors;
use clidispatch::output::new_logger;
use clidispatch::output::TermLogger;
use clidispatch::ReqCtx;
use cliparser::define_flags;
use configmodel::ConfigExt;
use migration::feature::deprecate;
use repo::repo::Repo;
use tracing::instrument;
use types::HgId;
use util::path::absolute;

use super::ConfigSet;
use super::Result;
use crate::HgPython;

static SEGMENTED_CHANGELOG_CAPABILITY: &str = "segmented-changelog";

define_flags! {
    pub struct CloneOpts {
        /// clone an empty working directory
        #[short('U')]
        noupdate: bool,

        /// revision or branch to check out
        #[short('u')]
        #[argtype("REV")]
        updaterev: String,

        /// include the specified changeset (DEPRECATED)
        #[short('r')]
        #[argtype("REV")]
        rev: String,

        /// use pull protocol to copy metadata (DEPRECATED)
        pull: bool,

        /// clone with minimal data processing (DEPRECATED)
        stream: bool,

        /// "use remotefilelog (only turn it off in legacy tests) (ADVANCED)"
        shallow: bool = true,

        /// "use git protocol (EXPERIMENTAL)"
        git: bool,

        /// enable a sparse profile
        enable_profile: Vec<String>,

        /// files to include in a sparse profile (DEPRECATED)
        include: String,

        /// files to exclude in a sparse profile (DEPRECATED)
        exclude: String,

        /// use EdenFs (EXPERIMENTAL)
        eden: bool,

        /// location of the backing repo to be used or created (EXPERIMENTAL)
        eden_backing_repo: String,

        #[arg]
        source: String,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(mut ctx: ReqCtx<CloneOpts>, config: &mut ConfigSet) -> Result<u8> {
    let mut logger = new_logger(ctx.io(), ctx.global_opts());

    let deprecated_options = [
        ("--rev", "rev-option", ctx.opts.rev.is_empty()),
        (
            "--include",
            "clone-include-option",
            ctx.opts.include.is_empty(),
        ),
        (
            "--exclude",
            "clone-exclude-option",
            ctx.opts.exclude.is_empty(),
        ),
    ];
    for (option_name, option_config, option_is_empty) in deprecated_options {
        if !option_is_empty {
            deprecate(
                config,
                option_config,
                format!("the {} option has been deprecated", option_name),
            )?;
        }
    }

    abort_if!(
        !ctx.opts.eden && !ctx.opts.eden_backing_repo.is_empty(),
        "--eden-backing-repo requires --eden",
    );

    abort_if!(
        !ctx.opts.enable_profile.is_empty() && ctx.opts.eden,
        "--enable-profile is not compatible with --eden",
    );

    abort_if!(
        ctx.opts.eden && ctx.opts.noupdate,
        "--noupdate is not compatible with --eden",
    );

    let force_rust = config
        .get_or_default::<Vec<String>>("commands", "force-rust")?
        .contains(&"clone".to_owned());
    let use_rust = force_rust || config.get_or_default("clone", "use-rust")?;
    if !use_rust {
        abort_if!(
            ctx.opts.eden,
            "--eden requires --config clone.use-rust=True"
        );

        logger.verbose("Falling back to Python clone (config not enabled)");
        return Err(errors::FallbackToPython("clone.use-rust not set to True".to_owned()).into());
    }

    let supported_url = match url::Url::parse(&ctx.opts.source) {
        Err(_) => false,
        Ok(url) => url.scheme() != "file" && url.scheme() != "ssh",
    };

    if !ctx.opts.updaterev.is_empty()
        || !ctx.opts.rev.is_empty()
        || ctx.opts.pull
        || ctx.opts.stream
        || !ctx.opts.shallow
        || ctx.opts.git
        || !supported_url
    {
        abort_if!(
            ctx.opts.eden,
            "some specified options are not compatible with --eden"
        );

        logger.verbose("Falling back to Python clone (incompatible options)");
        return Err(errors::FallbackToPython(
            "one or more unsupported options in Rust clone".to_owned(),
        )
        .into());
    }

    config.set(
        "paths",
        "default",
        Some(ctx.opts.source.clone()),
        &"arg".into(),
    );

    let reponame = match config.get_opt::<String>("remotefilelog", "reponame")? {
        // This gets the reponame from the --configfile config. Ingore
        // bogus "no-repo" value that dynamicconfig sets when there is
        // no repo name.
        Some(c) if c != "no-repo" => {
            logger.verbose(|| format!("Repo name is {} from config", c));
            c
        }
        Some(_) | None => match configloader::hg::repo_name_from_url(&ctx.opts.source) {
            Some(name) => {
                logger.verbose(|| format!("Repo name is {} via URL {}", name, ctx.opts.source));
                config.set(
                    "remotefilelog",
                    "reponame",
                    Some(&name),
                    &"clone source".into(),
                );
                name
            }
            None => abort!("could not determine repo name"),
        },
    };

    let destination = match ctx.opts.args.pop() {
        Some(dest) => absolute(dest).with_context(|| "Cannot get absolute destination path")?,
        None => {
            abort_if!(
                hgplain::is_plain(Some("default_clone_dir")),
                "DEST must be specified because HGPLAIN is enabled",
            );

            clone::get_default_destination_directory(config)?.join(&reponame)
        }
    };

    logger.info(format!(
        "Cloning {} into {}",
        reponame,
        destination.display(),
    ));

    let clone_type_str = if ctx.opts.eden {
        "eden_fs"
    } else if !ctx.opts.enable_profile.is_empty() {
        "sparse"
    } else {
        "full"
    };
    tracing::trace!("performing rust clone");
    tracing::debug!(target: "clone_info", rust_clone="true", repo=reponame, clone_type=clone_type_str, is_update_clone=!ctx.opts.noupdate);
    if !ctx.opts.enable_profile.is_empty() {
        tracing::debug!(target: "clone_info", cloned_sparse_profiles=ctx.opts.enable_profile.join(" "));
    }

    if let Some(ident) = identity::sniff_dir(&destination)? {
        abort!(
            "{} directory already exists at clone destination {}",
            ident.dot_dir(),
            destination.display(),
        );
    }

    if ctx.opts.eden {
        let backing_path = if !ctx.opts.eden_backing_repo.is_empty() {
            PathBuf::from(&ctx.opts.eden_backing_repo)
        } else if let Some(dir) = clone::get_default_eden_backing_directory(config)? {
            dir.join(&reponame)
        } else {
            abort!("please specify --eden-backing-repo");
        };

        let mut backing_repo = if identity::sniff_dir(&backing_path)?.is_none() {
            logger.verbose(|| {
                format!(
                    "Cloning {} backing repo to {}",
                    reponame,
                    backing_path.display(),
                )
            });
            try_clone_metadata(&ctx, &mut logger, config, &reponame, &backing_path)?
        } else {
            Repo::load(
                &backing_path,
                &ctx.global_opts().config,
                &ctx.global_opts().configfile,
            )?
        };
        let target_rev =
            get_update_target(&mut logger, &mut backing_repo, &ctx.opts)?.map(|(rev, _)| rev);
        logger.verbose(|| {
            format!(
                "Performing EdenFS clone {}@{} from {} to {}",
                reponame,
                target_rev.map_or(String::new(), |t| t.to_hex()),
                backing_path.display(),
                destination.display(),
            )
        });
        clone::eden_clone(&backing_repo, &destination, target_rev)?;
    } else {
        let mut repo = try_clone_metadata(&ctx, &mut logger, config, &reponame, &destination)?;

        let target_rev = get_update_target(&mut logger, &mut repo, &ctx.opts)?;
        if let Some((target_rev, bm)) = &target_rev {
            logger.info(format!("Checking out '{}'", bm));
            logger.verbose(|| {
                format!(
                    "Initializing non-EdenFS working copy to commit {}",
                    target_rev.to_hex(),
                )
            });
        } else {
            logger.verbose("Initializing empty non-EdenFS working copy");
        }

        clone::init_working_copy(
            &mut logger,
            &mut repo,
            target_rev.map(|(rev, _)| rev),
            ctx.opts.enable_profile.clone(),
        )?;
    }

    Ok(0)
}

fn try_clone_metadata(
    ctx: &ReqCtx<CloneOpts>,
    logger: &mut TermLogger,
    config: &mut ConfigSet,
    reponame: &str,
    destination: &Path,
) -> Result<Repo> {
    let dest_preexists = destination.exists();
    match clone_metadata(ctx, logger, config, reponame, destination) {
        Err(e) => {
            let removal_dir = if dest_preexists {
                let ident = identity::sniff_dir(destination)?.unwrap_or_else(identity::default);
                destination.join(ident.dot_dir())
            } else {
                destination.to_path_buf()
            };
            fs::remove_dir_all(removal_dir)?;
            Err(e)
        }
        Ok(repo) => Ok(repo),
    }
}

#[instrument(skip_all, fields(repo=reponame), err)]
fn clone_metadata(
    ctx: &ReqCtx<CloneOpts>,
    logger: &mut TermLogger,
    config: &mut ConfigSet,
    reponame: &str,
    destination: &Path,
) -> Result<Repo> {
    let mut includes = ctx.global_opts().configfile.clone();
    if let Some(mut repo_config) = config.get_opt::<PathBuf>("clone", "repo-specific-config-dir")? {
        repo_config.push(format!("{}.rc", reponame));
        if repo_config.exists() {
            let repo_config = repo_config.into_os_string().into_string().unwrap();
            if !includes.contains(&repo_config) {
                includes.push(repo_config);
            }
        }
    }

    let mut repo_config_file_content = includes
        .into_iter()
        .map(|file| format!("%include {}\n", file))
        .collect::<String>();
    repo_config_file_content
        .push_str(format!("\n[paths]\ndefault = {}\n", ctx.opts.source).as_str());

    let mut repo = Repo::init(
        destination,
        config,
        Some(repo_config_file_content),
        &ctx.global_opts().config,
    )?;
    repo.add_requirement("remotefilelog")?;

    let edenapi = repo.eden_api()?;

    let capabilities: Vec<String> =
        block_on(edenapi.capabilities())?.map_err(|e| e.tag_network())?;

    let segmented_changelog = capabilities
        .iter()
        .any(|cap| cap == SEGMENTED_CHANGELOG_CAPABILITY);

    if segmented_changelog {
        repo.add_store_requirement("lazychangelog")?;

        let bookmark_names: Vec<String> = get_selective_bookmarks(&repo)?;
        let metalog = repo.metalog()?;
        let commits = repo.dag_commits()?;
        tracing::trace!("fetching lazy commit data and bookmarks");
        let bookmark_ids = exchange::clone(
            edenapi,
            &mut metalog.write(),
            &mut commits.write(),
            bookmark_names,
        )?;
        logger.verbose(|| format!("Pulled bookmarks {:?}", bookmark_ids));
    } else {
        revlog_clone(repo.config(), logger, ctx, destination)?;
        // reload the repo to pick up any changes written out by the revlog clone
        // such as metalog remotenames writes
        repo = Repo::load(
            destination,
            &ctx.global_opts().config,
            &ctx.global_opts().configfile,
        )?;
    }

    ::fail::fail_point!("run::clone", |_| {
        abort!("Injected clone failure");
    });
    Ok(repo)
}

pub fn revlog_clone(
    config: &ConfigSet,
    logger: &mut TermLogger,
    ctx: &ReqCtx<CloneOpts>,
    root: &Path,
) -> Result<()> {
    let mut args = vec![
        identity::cli_name().to_string(),
        "debugrevlogclone".to_string(),
        ctx.opts.source.to_string(),
        "-R".to_string(),
        root.to_string_lossy().to_string(),
    ];

    for config in ctx.global_opts().config.iter() {
        args.push("--config".into());
        args.push(config.into());
    }
    if ctx.global_opts().quiet {
        args.push("-q".into());
    }
    if ctx.global_opts().verbose {
        args.push("-v".into());
    }
    if ctx.global_opts().debug {
        args.push("--debug".into());
    }

    logger.verbose(|| format!("Running {}", args.join(" ")));

    let hg_python = HgPython::new(&args);

    abort_if!(
        hg_python.run_hg(args, ctx.io(), config) != 0,
        "Cloning revlog failed"
    );
    Ok(())
}

fn get_selective_bookmarks(repo: &Repo) -> Result<Vec<String>> {
    match repo
        .config()
        .get_opt("remotenames", "selectivepulldefault")?
    {
        Some(bms) => Ok(bms),
        None => {
            abort!("remotenames.selectivepulldefault config is not set");
        }
    }
}

#[instrument(skip_all, err, ret)]
fn get_update_target(
    logger: &mut TermLogger,
    repo: &mut Repo,
    clone_opts: &CloneOpts,
) -> Result<Option<(HgId, String)>> {
    if clone_opts.noupdate {
        return Ok(None);
    }
    let selective_bookmarks = get_selective_bookmarks(repo)?;
    let main_bookmark = selective_bookmarks
        .first()
        .ok_or_else(|| {
            errors::Abort("remotenames.selectivepulldefault config list is empty".into())
        })?
        .clone();

    let remote_bookmark = exchange::convert_to_remote(&main_bookmark);
    let remote_bookmarks = repo.remote_bookmarks()?;

    match remote_bookmarks.get(&remote_bookmark) {
        Some(rev) => Ok(Some((rev.clone(), main_bookmark))),
        None => {
            logger.info(format!(
                "Server has no '{}' bookmark - skipping checkout.",
                remote_bookmark,
            ));
            Ok(None)
        }
    }
}

pub fn aliases() -> &'static str {
    "clone"
}

pub fn doc() -> &'static str {
    r#"make a copy of an existing repository

    Create a copy of an existing repository in a new directory.

    If no destination directory name is specified, it defaults to the
    basename of the source.

    The location of the source is added to the new repository's
    config file as the default to be used for future pulls.

    Sources are typically URLs. The following URL schemes are assumed
    to be a Git repo: ``git``, ``git+file``, ``git+ftp``, ``git+ftps``,
    ``git+http``, ``git+https``, ``git+ssh``, ``ssh`` and ``https``.

    Scp-like URLs of the form ``user@host:path`` are converted to
    ``ssh://user@host/path``.

    Other URL schemes are assumed to point to an EdenAPI capable repo.

    The ``--git`` option forces the source to be interpreted as a Git repo.

    To check out a particular version, use ``-u/--update``, or
    ``-U/--noupdate`` to create a clone with no working copy.

    If specified, the ``--enable-profile`` option should refer to a
    sparse profile within the source repo to filter the contents of
    the new working copy. See :prog:`help -e sparse` for details.

    .. container:: verbose

      Examples:

      - clone a remote repository to a new directory named some_repo::

          @prog@ clone https://example.com/some_repo

    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... SOURCE [DEST]")
}
