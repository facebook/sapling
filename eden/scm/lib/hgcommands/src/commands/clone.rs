/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use async_runtime::block_unless_interrupted as block_on;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::errors;
use clidispatch::fallback;
use clidispatch::ReqCtx;
use clidispatch::TermLogger;
use cliparser::define_flags;
use configloader::hg::resolve_custom_scheme;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::ValueSource;
use eagerepo::is_eager_repo;
use exchange::convert_to_remote;
use migration::feature::deprecate;
use repo::repo::Repo;
use repo_name::encode_repo_name;
use tracing::instrument;
use types::HgId;
use url::Url;
use util::file::atomic_write;
use util::path::absolute;
use util::path::create_shared_dir_all;

use super::ConfigSet;
use super::Result;
use crate::HgPython;

static SEGMENTED_CHANGELOG_CAPABILITY: &str = "segmented-changelog";
static COMMIT_GRAPH_SEGMENTS_CAPABILITY: &str = "commit-graph-segments";

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
        shallow: Option<bool>,

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

struct CloneSource {
    // Effective scheme, taking into account "schemes" config.
    scheme: String,
    // What should be used as paths.default.
    path: String,
    // Default bookmark (inferred from url fragment).
    default_bookmark: Option<String>,
}

impl CloneOpts {
    fn source(&self, config: &dyn Config) -> Result<CloneSource> {
        if let Some(local_path) = local_path(&self.source)? {
            let scheme = if is_eager_repo(&local_path) {
                "eager"
            } else {
                "file"
            };
            return Ok(CloneSource {
                scheme: scheme.to_string(),
                // Came from self.source, so should be UTF-8.
                path: local_path.into_os_string().into_string().unwrap(),
                default_bookmark: None,
            });
        }

        let mut url = Url::parse(&self.source)?;

        // Fragment is only used for choosing default bookmark during clone - we
        // don't want to persist it.
        let frag = url.fragment().map(|f| f.to_string());
        url.set_fragment(None);

        Ok(CloneSource {
            scheme: resolve_custom_scheme(config, url.clone())?
                .scheme()
                .to_string(),
            path: url.to_string(),
            default_bookmark: frag,
        })
    }
}

fn local_path(s: &str) -> Result<Option<PathBuf>> {
    if looks_like_windows_path(s)
        || matches!(Url::parse(s), Err(url::ParseError::RelativeUrlWithoutBase))
    {
        Ok(Some(util::path::absolute(s)?))
    } else {
        Ok(None)
    }
}

fn looks_like_windows_path(s: &str) -> bool {
    if !cfg!(windows) {
        return false;
    }

    // UNC prefix
    if s.starts_with(r"\\") {
        return true;
    }

    // Drive prefix (e.g. "c:")
    let bytes = s.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

pub fn run(mut ctx: ReqCtx<CloneOpts>, config: &mut ConfigSet) -> Result<u8> {
    let mut logger = ctx.logger();

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

    abort_if!(
        ctx.opts.eden && ctx.opts.shallow == Some(false),
        "--shallow is required with --eden",
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
        fallback!("clone.use-rust not set to True");
    }

    let source = match ctx.opts.source(config) {
        Err(_) => fallback!("invalid URL"),
        Ok(source) => match source.scheme.as_ref() {
            "mononoke" | "eager" | "test" => source,
            _ => fallback!("unsupported URL scheme"),
        },
    };

    if !ctx.opts.rev.is_empty()
        || ctx.opts.pull
        || ctx.opts.stream
        || ctx.opts.git
        // Allow Rust clone to handle --updaterev if experimental.rust-clone-updaterev is set.
        || (!ctx.opts.updaterev.is_empty() && !config.get_or_default("experimental", "rust-clone-updaterev")?)
    {
        abort_if!(
            ctx.opts.eden,
            "some specified options are not compatible with --eden"
        );

        logger.verbose("Falling back to Python clone (incompatible options)");
        fallback!("one or more unsupported options in Rust clone");
    }

    config.set("paths", "default", Some(&source.path), &"arg".into());

    let reponame = match config.get_opt::<String>("remotefilelog", "reponame")? {
        // This gets the reponame from the --configfile config. Ignore
        // bogus "no-repo" value that internalconfig sets when there is
        // no repo name.
        Some(c) if c != "no-repo" => {
            logger.verbose(|| format!("Repo name is {} from config", c));
            c
        }
        Some(_) | None => match configloader::hg::repo_name_from_url(config, &ctx.opts.source) {
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

            // Change "some/repo" into "repo". There is an argument to
            // defaulting to "some_repo" or similar if the canonical repo name
            // contains a slash, but using just "repo" probably has less
            // friction with current workflows/expectations.
            let basename = match reponame.split(&['/', '\\']).last() {
                Some(name) if !name.is_empty() => name,
                _ => abort!("invalid reponame {reponame}"),
            };

            clone::get_default_destination_directory(config)?.join(basename)
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
            dir.join(encode_repo_name(&reponame))
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

        let target_rev = match get_update_target(&mut logger, &mut repo, &ctx.opts)? {
            Some((id, name)) => {
                logger.info(format!("Checking out '{}'", name));

                logger.verbose(|| {
                    format!(
                        "Initializing non-EdenFS working copy to commit {}",
                        id.to_hex(),
                    )
                });

                Some(id)
            }
            None => {
                logger.verbose("Initializing empty non-EdenFS working copy");
                None
            }
        };

        clone::init_working_copy(
            &mut logger,
            &mut repo,
            target_rev,
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
            if !ctx.global_opts().debug {
                fs::remove_dir_all(removal_dir)?;
            }
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
        repo_config.push(format!("{}.rc", encode_repo_name(reponame)));
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

    if !repo_config_file_content.is_empty() {
        repo_config_file_content.push('\n');
    }

    let source = ctx.opts.source(config)?;
    if let Some(bm) = &source.default_bookmark {
        config.set(
            "remotenames",
            "selectivepulldefault",
            Some(bm),
            &"clone source".into(),
        );
    }

    repo_config_file_content.push_str(format!("[paths]\ndefault = {}\n", source.path).as_str());

    // Some config values are inherent to the repo and should be persisted if passed to clone.
    // This is analagous to persisting the --configfile args above.
    for (section, name) in &[("remotenames", "selectivepulldefault")] {
        if let Some(&ValueSource {
            ref source,
            value: Some(ref value),
            ..
        }) = config.get_sources(section, name).last()
        {
            if *source == "--config" || *source == "clone source" {
                repo_config_file_content.push_str(&format!("\n[{section}]\n{name} = {value}\n"));
            }
        }
    }

    let eager_format: bool = config.get_or_default("format", "use-eager-repo")?;

    let shallow = match ctx.opts.shallow {
        Some(shallow) => shallow,
        // Infer non-shallow for eager->eager clone.
        None => !eager_format || source.scheme != "eager",
    };

    if shallow {
        config.set("format", "use-remotefilelog", Some("true"), &"clone".into());
    } else {
        if !eager_format {
            fallback!("non-shallow && non-eagerepo");
        }

        abort_if!(
            source.scheme != "eager",
            "don't know how to clone {} into eagerepo",
            source.path,
        );

        return eager_clone(ctx, config, source, destination);
    }

    let mut repo = Repo::init(
        destination,
        config,
        Some(repo_config_file_content),
        &ctx.global_opts().config,
    )?;

    let edenapi = repo.eden_api()?;

    let capabilities: Vec<String> =
        block_on(edenapi.capabilities())?.map_err(|e| e.tag_network())?;

    let segmented_changelog = capabilities
        .iter()
        .any(|cap| cap == SEGMENTED_CHANGELOG_CAPABILITY);
    let commit_graph_segments = capabilities
        .iter()
        .any(|cap| cap == COMMIT_GRAPH_SEGMENTS_CAPABILITY)
        && repo
            .config()
            .get_or_default::<bool>("clone", "use-commit-graph")?;

    let mut repo_needs_reload = false;

    if segmented_changelog || commit_graph_segments {
        repo.add_store_requirement("lazychangelog")?;

        let bookmark_names: Vec<String> = get_selective_bookmarks(&repo)?;
        let metalog = repo.metalog()?;
        let commits = repo.dag_commits()?;
        tracing::trace!("fetching lazy commit data and bookmarks");
        let bookmark_ids = exchange::clone(
            repo.config(),
            edenapi,
            &mut metalog.write(),
            &mut commits.write(),
            bookmark_names,
        )?;
        logger.verbose(|| format!("Pulled bookmarks {:?}", bookmark_ids));

        if repo
            .config()
            .get_or_default("devel", "segmented-changelog-rev-compat")?
        {
            // "lazytext" (vs "lazy") is required for rev compat mode, so let's
            // migrate automatically. This migration only works for tests.
            migrate_to_lazytext(ctx, repo.config(), repo.path())?;
            repo_needs_reload = true;
        }
    } else {
        revlog_clone(repo.config(), logger, ctx, destination)?;
        // reload the repo to pick up any changes written out by the revlog clone
        // such as metalog remotenames writes
        repo_needs_reload = true;
    }

    if repo_needs_reload {
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

fn eager_clone(
    ctx: &ReqCtx<CloneOpts>,
    config: &ConfigSet,
    source: CloneSource,
    dest: &Path,
) -> Result<Repo> {
    let source_path = eagerepo::EagerRepo::url_to_dir(&source.path)
        .ok_or_else(|| anyhow!("no eagerepo at {}", source.path))?;
    let source_dot_dir = source_path.join(identity::must_sniff_dir(&source_path)?.dot_dir());

    let dest_ident = identity::default();
    let dest_dot_dir = dest.join(dest_ident.dot_dir());

    // Copy over store files.
    recursive_copy(&source_dot_dir.join("store"), &dest_dot_dir.join("store"))?;
    // Init working copy.
    eagerepo::EagerRepo::open(dest)?;

    let config_path = dest_dot_dir.join(dest_ident.config_repo_file());
    atomic_write(&config_path, |f| {
        f.write_all(format!("[paths]\ndefault = {}\n", source.path).as_bytes())
    })?;

    let mut repo = Repo::load(
        dest,
        &ctx.global_opts().config,
        &ctx.global_opts().configfile,
    )?;

    // Convert bookmarks to remotenames.
    let remote_names: BTreeMap<String, HgId> = repo
        .local_bookmarks()?
        .iter()
        .map(|(bm, id)| Ok((convert_to_remote(config, bm)?, id.clone())))
        .collect::<Result<_>>()?;

    repo.set_remote_bookmarks(&remote_names)?;

    let ml = repo.metalog()?;
    let mut ml = ml.write();
    ml.set("bookmarks", b"")?;
    let mut opts = metalog::CommitOptions::default();
    opts.message = "eager clone";
    ml.commit(opts)?;

    Ok(repo)
}

fn recursive_copy(from: &Path, to: &Path) -> Result<()> {
    create_shared_dir_all(to)?;

    for entry in fs::read_dir(from)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            recursive_copy(&entry.path(), &to.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), to.join(entry.file_name()))?;
        }
    }

    Ok(())
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
        ctx.opts.source(config)?.path,
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

fn migrate_to_lazytext(ctx: &ReqCtx<CloneOpts>, config: &ConfigSet, root: &Path) -> Result<()> {
    let args = vec![
        identity::cli_name().to_string(),
        "debugchangelog".to_string(),
        "--migrate".to_string(),
        "lazytext".to_string(),
        "-R".to_string(),
        root.to_str().unwrap().to_string(),
    ];

    let hg_python = HgPython::new(&args);
    abort_if!(
        hg_python.run_hg(args, ctx.io(), config) != 0,
        "rev compat lazytext migration failed"
    );

    Ok(())
}

fn get_selective_bookmarks(repo: &Repo) -> Result<Vec<String>> {
    Ok(repo
        .config()
        .must_get("remotenames", "selectivepulldefault")?)
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

    if !clone_opts.updaterev.is_empty() {
        return Ok(Some((
            repo.resolve_commit(None, &clone_opts.updaterev)?,
            clone_opts.updaterev.clone(),
        )));
    }

    let selective_bookmarks = get_selective_bookmarks(repo)?;
    let main_bookmark = selective_bookmarks
        .first()
        .ok_or_else(|| {
            errors::Abort("remotenames.selectivepulldefault config list is empty".into())
        })?
        .clone();

    match repo.resolve_commit_opt(None, &main_bookmark)? {
        Some(id) => Ok(Some((id, main_bookmark))),
        None => {
            logger.info(format!(
                "Server has no '{}' bookmark - trying tip.",
                main_bookmark,
            ));

            if let Some(tip) = repo.resolve_commit_opt(None, "tip")? {
                return Ok(Some((tip, "tip".to_string())));
            }

            logger.info("Skipping checkout - no commits available.".to_string());

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

    .. container:: verbose

      As an experimental feature, if specified the source URL fragment
      is persisted as the repo's main bookmark.

    Returns 0 on success."#
}

pub fn synopsis() -> Option<&'static str> {
    Some("[OPTION]... SOURCE [DEST]")
}
