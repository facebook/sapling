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
use clidispatch::global_flags::HgGlobalOpts;
use clidispatch::output::new_logger;
use clidispatch::output::TermLogger;
use cliparser::define_flags;
use migration::feature::deprecate;
use repo::constants::HG_PATH;
use repo::repo::Repo;
use tracing::instrument;
use types::HgId;
use util::path::absolute;

use super::ConfigSet;
use super::Result;
use super::IO;

use crate::HgPython;

static SEGMENTED_CHANGELOG_CAPABILITY: &str = "segmented-changelog";

define_flags! {
    pub struct CloneOpts {
        /// clone an empty working directory
        #[short('U')]
        noupdate: bool,

        /// revision or branch to check out
        #[short('u')]
        updaterev: String,

        /// include the specified changeset (DEPRECATED)
        #[short('r')]
        rev: String,

        /// use pull protocol to copy metadata
        pull: bool,

        /// clone with minimal data processing
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

pub fn run(
    mut clone_opts: CloneOpts,
    global_opts: HgGlobalOpts,
    io: &IO,
    config: &mut ConfigSet,
) -> Result<u8> {
    let mut logger = new_logger(io, &global_opts);

    let deprecated_options = [
        ("--rev", "rev-option", clone_opts.rev.is_empty()),
        (
            "--include",
            "clone-include-option",
            clone_opts.include.is_empty(),
        ),
        (
            "--exclude",
            "clone-exclude-option",
            clone_opts.exclude.is_empty(),
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
        !clone_opts.eden && !clone_opts.eden_backing_repo.is_empty(),
        "--eden-backing-repo requires --eden",
    );

    abort_if!(
        !clone_opts.enable_profile.is_empty() && clone_opts.eden,
        "--enable-profile is not compatible with --eden",
    );

    abort_if!(
        clone_opts.eden && clone_opts.noupdate,
        "--noupdate is not compatible with --eden",
    );

    let force_rust = config
        .get_or_default::<Vec<String>>("commands", "force-rust")?
        .contains(&name().to_owned());
    let use_rust = force_rust || config.get_or_default("clone", "use-rust")?;
    if !use_rust {
        abort_if!(
            clone_opts.eden,
            "--eden requires --config clone.use-rust=True"
        );

        logger.info("Falling back to Python clone (config not enabled)");
        return Err(errors::FallbackToPython(name()).into());
    }

    let supported_url = match url::Url::parse(&clone_opts.source) {
        Err(_) => false,
        Ok(url) => url.scheme() != "file" && url.scheme() != "ssh",
    };

    if !clone_opts.updaterev.is_empty()
        || !clone_opts.rev.is_empty()
        || clone_opts.pull
        || clone_opts.stream
        || !clone_opts.shallow
        || clone_opts.git
        || !supported_url
    {
        abort_if!(
            clone_opts.eden,
            "some specified options are not compatible with --eden"
        );

        logger.info("Falling back to Python clone (incompatible options)");
        return Err(errors::FallbackToPython(name()).into());
    }

    config.set(
        "paths",
        "default",
        Some(clone_opts.source.clone()),
        &"arg".into(),
    );

    let reponame = match config.get_opt::<String>("remotefilelog", "reponame")? {
        // This gets the reponame from the --configfile config. Ingore
        // bogus "no-repo" value that dynamicconfig sets when there is
        // no repo name.
        Some(c) if c != "no-repo" => {
            logger.debug(|| format!("Repo name is {} from config", c));
            c
        }
        Some(_) | None => match configparser::hg::repo_name_from_url(&clone_opts.source) {
            Some(name) => {
                logger.debug(|| format!("Repo name is {} via URL {}", name, clone_opts.source));
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

    let destination = match clone_opts.args.pop() {
        Some(dest) => absolute(dest).with_context(|| "Cannot get absolute destination path")?,
        None => {
            abort_if!(
                configparser::hg::is_plain(Some("default_clone_dir")),
                "DEST must be specified because HGPLAIN is enabled",
            );

            clone::get_default_destination_directory(config)?.join(&reponame)
        }
    };

    logger.status(format!(
        "Cloning {} into {}",
        reponame,
        destination.display(),
    ));

    let dest_hg = destination.join(HG_PATH);

    abort_if!(
        dest_hg.exists(),
        ".hg directory already exists at clone destination {}",
        destination.display(),
    );

    if clone_opts.eden {
        let backing_path = if !clone_opts.eden_backing_repo.is_empty() {
            PathBuf::from(&clone_opts.eden_backing_repo)
        } else if let Some(dir) = clone::get_default_eden_backing_directory(config)? {
            dir.join(&reponame)
        } else {
            abort!("please specify --eden-backing-repo");
        };
        let backing_hg = backing_path.join(".hg");

        let mut backing_repo = if !backing_hg.exists() {
            logger.info(|| {
                format!(
                    "Cloning {} backing repo to {}",
                    reponame,
                    backing_path.display(),
                )
            });
            try_clone_metadata(
                &mut logger,
                io,
                &clone_opts,
                &global_opts,
                config,
                &reponame,
                &backing_path,
            )?
        } else {
            Repo::load(&backing_path, &global_opts.config, &global_opts.configfile)?
        };
        let target_rev =
            get_update_target(&mut logger, &mut backing_repo, &clone_opts)?.map(|(rev, _)| rev);
        logger.info(|| {
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
        let mut repo = try_clone_metadata(
            &mut logger,
            io,
            &clone_opts,
            &global_opts,
            config,
            &reponame,
            &destination,
        )?;

        let target_rev = get_update_target(&mut logger, &mut repo, &clone_opts)?;
        if let Some((target_rev, bm)) = &target_rev {
            logger.status(format!("Checking out '{}'", bm));
            logger.info(|| {
                format!(
                    "Initializing non-EdenFS working copy to commit {}",
                    target_rev.to_hex(),
                )
            });
        } else {
            logger.info("Initializing empty non-EdenFS working copy");
        }

        clone::init_working_copy(
            &mut logger,
            &mut repo,
            target_rev.map(|(rev, _)| rev),
            clone_opts.enable_profile.clone(),
        )?;
    }

    Ok(0)
}

fn try_clone_metadata(
    logger: &mut TermLogger,
    io: &IO,
    clone_opts: &CloneOpts,
    global_opts: &HgGlobalOpts,
    config: &mut ConfigSet,
    reponame: &str,
    destination: &Path,
) -> Result<Repo> {
    let dest_preexists = destination.exists();
    match clone_metadata(
        logger,
        io,
        clone_opts,
        global_opts,
        config,
        reponame,
        destination,
    ) {
        Err(e) => {
            let removal_dir = if dest_preexists {
                destination.join(HG_PATH)
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
    logger: &mut TermLogger,
    io: &IO,
    clone_opts: &CloneOpts,
    global_opts: &HgGlobalOpts,
    config: &mut ConfigSet,
    reponame: &str,
    destination: &Path,
) -> Result<Repo> {
    tracing::trace!("performing rust clone");
    tracing::debug!(target: "rust_clone", rust_clone="true");

    let mut includes = global_opts.configfile.clone();
    if let Some(mut repo_config) = config.get_opt::<PathBuf>("clone", "repo-specific-config-dir")? {
        repo_config.push(format!("{}.rc", reponame));
        if repo_config.exists() {
            let repo_config = repo_config.into_os_string().into_string().unwrap();
            if !includes.contains(&repo_config) {
                includes.push(repo_config);
            }
        }
    }

    let mut hgrc_content = includes
        .into_iter()
        .map(|file| format!("%include {}\n", file))
        .collect::<String>();
    hgrc_content.push_str(format!("\n[paths]\ndefault = {}\n", clone_opts.source).as_str());

    let mut repo = Repo::init(destination, config, Some(hgrc_content), &global_opts.config)?;
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
        logger.debug(|| format!("Pulled bookmarks {:?}", bookmark_ids));
    } else {
        revlog_clone(logger, io, global_opts, &clone_opts.source, destination)?;
        // reload the repo to pick up any changes written out by the revlog clone
        // such as metalog remotenames writes
        repo = Repo::load(destination, &global_opts.config, &global_opts.configfile)?;
    }

    ::fail::fail_point!("run::clone", |_| {
        abort!("Injected clone failure");
    });
    Ok(repo)
}

pub fn revlog_clone(
    logger: &mut TermLogger,
    io: &IO,
    global_opts: &HgGlobalOpts,
    source: &str,
    root: &Path,
) -> Result<()> {
    let mut args = vec![
        "hg".to_string(),
        "debugrevlogclone".to_string(),
        source.to_string(),
        "-R".to_string(),
        root.to_string_lossy().to_string(),
    ];

    for config in global_opts.config.iter() {
        args.push("--config".into());
        args.push(config.into());
    }
    if global_opts.quiet {
        args.push("-q".into());
    }
    if global_opts.verbose {
        args.push("-v".into());
    }
    if global_opts.debug {
        args.push("--debug".into());
    }

    logger.debug(|| format!("Running {}", args.join(" ")));

    let hg_python = HgPython::new(&args);

    abort_if!(hg_python.run_hg(args, io) != 0, "Cloning revlog failed");
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
            logger.status(format!(
                "Server has no '{}' bookmark - skipping checkout.",
                remote_bookmark,
            ));
            Ok(None)
        }
    }
}

pub fn name() -> &'static str {
    "clone"
}

pub fn doc() -> &'static str {
    r#"make a copy of an existing repository

    Create a copy of an existing repository in a new directory.

    If no destination directory name is specified, it defaults to the
    basename of the source.

    The location of the source is added to the new repository's
    ``.hg/hgrc`` file, as the default to be used for future pulls.

    Only local paths and ``ssh://`` URLs are supported as
    destinations. For ``ssh://`` destinations, no working directory or
    ``.hg/hgrc`` will be created on the remote side.

    If the source repository has a bookmark called '@' set, that
    revision will be checked out in the new repository by default.

    To check out a particular version, use -u/--update, or
    -U/--noupdate to create a clone with no working directory.

    To pull only a subset of changesets, specify one or more revisions
    identifiers with -r/--rev. The resulting clone will contain only the
    specified changesets and their ancestors. These options (or 'clone src#rev
    dest') imply --pull, even for local source repositories.

    In normal clone mode, the remote normalizes repository data into a common
    exchange format and the receiving end translates this data into its local
    storage format. --stream activates a different clone mode that essentially
    copies repository files from the remote with minimal data processing. This
    significantly reduces the CPU cost of a clone both remotely and locally.
    However, it often increases the transferred data size by 30-40%. This can
    result in substantially faster clones where I/O throughput is plentiful,
    especially for larger repositories. A side-effect of --stream clones is
    that storage settings and requirements on the remote are applied locally:
    a modern client may inherit legacy or inefficient storage used by the
    remote or a legacy Mercurial client may not be able to clone from a
    modern Mercurial remote.

    .. container:: verbose

      For efficiency, hardlinks are used for cloning whenever the
      source and destination are on the same filesystem (note this
      applies only to the repository data, not to the working
      directory). Some filesystems, such as AFS, implement hardlinking
      incorrectly, but do not report errors. In these cases, use the
      --pull option to avoid hardlinking.

      Mercurial will update the working directory to the first applicable
      revision from this list:

      a) null if -U or the source repository has no changesets
      b) if -u . and the source repository is local, the first parent of
         the source repository's working directory
      c) the changeset specified with -u (if a branch name, this means the
         latest head of that branch)
      d) the changeset specified with -r
      e) the tipmost head specified with -b
      f) the tipmost head specified with the url#branch source syntax
      g) the revision marked with the '@' bookmark, if present
      h) the tipmost head of the default branch
      i) tip

      When cloning from servers that support it, Mercurial may fetch
      pre-generated data from a server-advertised URL. When this is done,
      hooks operating on incoming changesets and changegroups may fire twice,
      once for the bundle fetched from the URL and another for any additional
      data not fetched from this URL. In addition, if an error occurs, the
      repository may be rolled back to a partial clone. This behavior may
      change in future releases. See :hg:`help -e clonebundles` for more.

      Examples:

      - clone a remote repository to a new directory named hg/::

          hg clone https://www.mercurial-scm.org/repo/hg/

      - create a lightweight local clone::

          hg clone project/ project-feature/

      - clone from an absolute path on an ssh server (note double-slash)::

          hg clone ssh://user@server//home/projects/alpha/

      - do a streaming clone while checking out a specified version::

          hg clone --stream http://server/repo -u 1.5

      - create a repository without changesets after a particular revision::

          hg clone -r 04e544 experimental/ good/

      - clone (and track) a particular named branch::

          hg clone https://www.mercurial-scm.org/repo/hg/#stable

    See :hg:`help urls` for details on specifying URLs.

    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... SOURCE [DEST]")
}
