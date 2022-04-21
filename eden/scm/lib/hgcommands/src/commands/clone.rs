/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_runtime::block_unless_interrupted as block_on;
use clidispatch::errors;
use cliparser::define_flags;
use edenapi::Builder;

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

        /// include the specified changeset
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
        enable_profile: String,

        /// files to include in a sparse profile
        include: String,

        /// files to exclude in a sparse profile
        exclude: String,

        #[arg]
        source: String,

        #[args]
        args: Vec<String>,
    }
}

pub fn run(opts: CloneOpts, _io: &IO, mut config: ConfigSet) -> Result<u8> {
    if !config.get_or_default::<bool>("clone", "use-rust")? {
        return Err(errors::FallbackToPython.into());
    }

    if !opts.noupdate
        || !opts.updaterev.is_empty()
        || !opts.rev.is_empty()
        || opts.pull
        || opts.stream
        || !opts.shallow
        || opts.git
        || !opts.enable_profile.is_empty()
        || !opts.include.is_empty()
        || !opts.exclude.is_empty()
    {
        return Err(errors::FallbackToPython.into());
    }

    // Rust clone only supports segmented changelog clone
    // TODO: add binding for python streaming revlog download
    config.set("paths", "default", Some(opts.source.clone()), &"arg".into());
    let edenapi = Builder::from_config(&config)?
        .correlator(Some(edenapi::DEFAULT_CORRELATOR.as_str()))
        .build()?;
    let capabilities: Vec<String> = block_on(edenapi.capabilities())??;
    if !capabilities
        .iter()
        .any(|cap| cap == SEGMENTED_CHANGELOG_CAPABILITY)
    {
        return Err(errors::FallbackToPython.into());
    }

    clone(opts, config)?;
    Ok(0)
}

fn clone(mut _opts: CloneOpts, mut _config: ConfigSet) -> Result<u8> {
    Err(errors::FallbackToPython.into())
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
