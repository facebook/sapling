/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use async_runtime::block_unless_interrupted as block_on;
use clidispatch::errors;
use clidispatch::global_flags::HgGlobalOpts;
use cliparser::define_flags;
use edenapi::Builder;
use migration::feature::deprecate;
use repo::constants::HG_PATH;
use repo::repo::Repo;
use types::HgId;

use super::ConfigSet;
use super::Result;
use super::IO;

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

    let force_rust = config
        .get_or_default::<Vec<String>>("commands", "force-rust")?
        .contains(&name().to_owned());
    let use_rust = force_rust || config.get_or_default("clone", "use-rust")?;
    if !use_rust {
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
        || clone_opts.eden
        || !supported_url
    {
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
        Some(c) if c != "no-repo" => c,
        Some(_) | None => match configparser::hg::repo_name_from_url(&clone_opts.source) {
            Some(name) => {
                config.set(
                    "remotefilelog",
                    "reponame",
                    Some(&name),
                    &"clone source".into(),
                );
                name
            }
            None => return Err(errors::Abort("could not determine repo name".into()).into()),
        },
    };

    // Rust clone only supports segmented changelog clone
    // TODO: add binding for python streaming revlog download
    let edenapi = Builder::from_config(config)?
        .correlator(Some(edenapi::DEFAULT_CORRELATOR.as_str()))
        .build()?;
    let capabilities: Vec<String> =
        block_on(edenapi.capabilities())?.map_err(|e| e.tag_network())?;
    if !capabilities
        .iter()
        .any(|cap| cap == SEGMENTED_CHANGELOG_CAPABILITY)
    {
        return Err(errors::FallbackToPython(name()).into());
    }

    let destination = match clone_opts.args.pop() {
        Some(dest) => PathBuf::from(dest),
        None => {
            if configparser::hg::is_plain(Some("default_clone_dir")) {
                return Err(errors::Abort("DEST was not specified".into()).into());
            } else {
                clone::get_default_destination_directory(config)?.join(&reponame)
            }
        }
    };

    let dest_preexists = destination.exists();
    let dest_hg = destination.join(HG_PATH);
    if dest_hg.exists() {
        return Err(
            errors::Abort(".hg directory already exists at clone destination".into()).into(),
        );
    }
    match clone_metadata(&clone_opts, &global_opts, config, &destination, &reponame) {
        Ok(mut repo) => {
            if let Some(target_rev) = get_update_target(io, &mut repo, &clone_opts, &global_opts)? {
                let mut repo = repo;
                clone::init_working_copy(&mut repo, target_rev, clone_opts.enable_profile.clone())?;
            }
        }
        Err(e) => {
            let removal_dir = if dest_preexists {
                destination.join(HG_PATH)
            } else {
                destination
            };
            fs::remove_dir_all(removal_dir)?;
            return Err(e);
        }
    }
    Ok(0)
}

fn clone_metadata(
    clone_opts: &CloneOpts,
    global_opts: &HgGlobalOpts,
    config: &mut ConfigSet,
    destination: &Path,
    reponame: &str,
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

    let mut repo = Repo::init(destination, config, Some(hgrc_content))?;

    repo.config_mut().set_overrides(&global_opts.config)?;
    repo.add_store_requirement("lazychangelog")?;
    repo.add_requirement("remotefilelog")?;

    let edenapi = repo.eden_api()?;
    let metalog = repo.metalog()?;
    let commits = repo.dag_commits()?;

    let bookmark_names: Vec<String> = get_selective_bookmarks(&repo)?;

    tracing::trace!("fetching lazy commit data and bookmarks");
    exchange::clone(
        edenapi,
        &mut metalog.write(),
        &mut commits.write(),
        bookmark_names.clone(),
    )?;

    ::fail::fail_point!("run::clone", |_| {
        Err(errors::Abort("Injected clone failure".to_string().into()).into())
    });
    Ok(repo)
}

fn get_selective_bookmarks(repo: &Repo) -> Result<Vec<String>> {
    match repo
        .config()
        .get_opt("remotenames", "selectivepulldefault")?
    {
        Some(bms) => Ok(bms),
        None => {
            Err(errors::Abort("remotenames.selectivepulldefault config is not set".into()).into())
        }
    }
}

fn get_update_target(
    io: &IO,
    repo: &mut Repo,
    clone_opts: &CloneOpts,
    global_opts: &HgGlobalOpts,
) -> Result<Option<HgId>> {
    if clone_opts.noupdate {
        return Ok(None);
    }
    let selective_bookmarks = get_selective_bookmarks(repo)?;
    let main_bookmark = selective_bookmarks
        .first()
        .ok_or_else(|| {
            errors::Abort("remotenames.selectivepulldefault config list is empty".into())
        })
        .map(|bm| exchange::convert_to_remote(bm))?;
    let remote_bookmarks = repo.remote_bookmarks()?;

    match remote_bookmarks.get(&main_bookmark) {
        Some(rev) => Ok(Some(rev.clone())),
        None => {
            if !global_opts.quiet {
                write!(
                    io.error(),
                    "Server has no '{}' bookmark - skipping checkout.\n",
                    main_bookmark
                )?;
            }
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
