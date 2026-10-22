/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `config --user/--local/--system [--delete] NAME...`: edit config files.

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use clidispatch::ReqCtx;
use clidispatch::abort;
use clidispatch::abort_if;
use clidispatch::fallback;
use cmdutil::Repo;
use cmdutil::Result;
use identity::Identity;
use util::errors::IOContext;

use crate::ConfigOpts;

pub(crate) fn run(ctx: &ReqCtx<ConfigOpts>, repo: Option<&Repo>) -> Result<u8> {
    let target = Target::from_opts(&ctx.opts)?;
    let ident = repo.map(Repo::ident).unwrap_or_else(identity::default);
    let paths = target.paths(ident, repo)?;

    if ctx.opts.args.is_empty() {
        fallback!("opening an editor for config edits is not supported in Rust");
    }
    let edits = parse_edits(&ctx.opts.args, ctx.opts.delete)?;

    let path = paths.iter().find(|p| p.exists()).unwrap_or(&paths[0]);
    if !path.exists() {
        write_sample_config(path, target, ident)?;
    }

    let logger = ctx.logger();
    for Edit {
        section,
        name,
        value,
    } in edits
    {
        match value {
            Some(value) => logger
                .verbose(|| format!("setting {section}.{name}={value} in {}\n", path.display())),
            None => {
                logger.verbose(|| format!("deleting {section}.{name} from {}\n", path.display()))
            }
        }
        let outcome = rcutil::edit_config(path, section, name, value)?;
        if outcome.definitions > 1 && !ctx.global_opts().quiet {
            logger.warn(format!(
                "warning: duplicate config entries for {section}.{name} in {}\n",
                path.display()
            ));
        }
    }
    logger.info(|| format!("updated config in {}\n", path.display()));
    Ok(0)
}

/// Which config file to edit.
#[derive(Clone, Copy)]
enum Target {
    User,
    Local,
    System,
}

impl Target {
    fn from_opts(opts: &ConfigOpts) -> Result<Self> {
        let selected: Vec<Target> = [
            (opts.user, Target::User),
            (opts.local, Target::Local),
            (opts.system, Target::System),
            (opts.global, Target::System),
        ]
        .into_iter()
        .filter_map(|(selected, target)| selected.then_some(target))
        .collect();
        match selected[..] {
            [target] => Ok(target),
            [] if opts.edit => Ok(Target::User),
            [] if opts.delete => abort!("--delete requires one of --user, --local or --system"),
            _ => abort!("please specify exactly one config location"),
        }
    }

    /// Candidate config files, most preferred first. Never empty.
    fn paths(self, ident: Identity, repo: Option<&Repo>) -> Result<Vec<PathBuf>> {
        let paths = match self {
            Target::Local => match repo {
                Some(repo) => vec![repo.config_path()],
                None => abort!("can't use --local outside a repository"),
            },
            Target::User => ident.user_config_paths(),
            Target::System => ident.system_config_paths(),
        };
        abort_if!(
            paths.is_empty(),
            "can't determine {} config path",
            self.name()
        );
        Ok(paths)
    }

    fn name(self) -> &'static str {
        match self {
            Target::User => "user",
            Target::Local => "local",
            Target::System => "system",
        }
    }

    /// Commented example content for a config file that does not exist yet.
    fn sample(self) -> &'static str {
        match self {
            Target::User => {
                r#"# example user config (see '@prog@ help config' for more info)
[ui]
# name and email, e.g.
# username = Jane Doe <jdoe@example.com>

# uncomment to disable color in command output
# (see '@prog@ help color' for details)
# color = never

# uncomment to disable command output pagination
# (see '@prog@ help pager' for details)
# paginate = never
"#
            }
            Target::Local => {
                r#"# example repository config (see '@prog@ help config' for more info)
[paths]
# URL aliases to other repo sources
# (see '@prog@ help config.paths' for more info)
#
# default = https://example.com/example-org/example-repo
# my-fork = ssh://jdoe@example.com/jdoe/example-repo

[ui]
# name and email (local to this repository, optional), e.g.
# username = Jane Doe <jdoe@example.com>
"#
            }
            Target::System => {
                r#"# example system-wide @prog@ config (see '@prog@ help config' for more info)

[ui]
# uncomment to disable color in command output
# (see '@prog@ help color' for details)
# color = never

# uncomment to disable command output pagination
# (see '@prog@ help pager' for details)
# paginate = never
"#
            }
        }
    }
}

fn write_sample_config(path: &Path, target: Target, ident: Identity) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).path_context("cannot create config directory", dir)?;
    }
    let sample = ident.punch(target.sample());
    let sample = if cfg!(windows) {
        sample.replace('\n', "\r\n")
    } else {
        sample
    };
    fs::write(path, sample).path_context("cannot create config file", path)?;
    Ok(())
}

/// One `section.name` to set to `value`, or to delete when `value` is None.
struct Edit<'a> {
    section: &'a str,
    name: &'a str,
    value: Option<&'a str>,
}

/// Parse `section.name=value` and `section.name value` arguments, or
/// `section.name` arguments when deleting.
fn parse_edits(args: &[String], delete: bool) -> Result<Vec<Edit<'_>>> {
    let mut edits = Vec::new();
    let mut pending_key: Option<&str> = None;
    let mut invalid: Option<&str> = None;
    for arg in args {
        let (key, value) = match pending_key.take() {
            Some(key) => (key, Some(arg.as_str())),
            None => match arg.split_once('=') {
                Some(_) if delete => {
                    invalid = Some(arg);
                    break;
                }
                Some((key, value)) => (key, Some(value)),
                None if delete => (arg.as_str(), None),
                None => {
                    pending_key = Some(arg);
                    continue;
                }
            },
        };
        let Some((section, name)) = key.split_once('.') else {
            invalid = Some(key);
            break;
        };
        edits.push(Edit {
            section,
            name,
            value,
        });
    }
    if let Some(arg) = invalid.or(pending_key) {
        if delete {
            abort!("invalid config deletion: '{arg}'\n(try section.name)");
        } else {
            abort!("invalid config edit: '{arg}'\n(try section.name=value)");
        }
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], delete: bool) -> Result<Vec<(String, String, Option<String>)>> {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let edits = parse_edits(&args, delete)?;
        Ok(edits
            .iter()
            .map(|e| {
                (
                    e.section.to_string(),
                    e.name.to_string(),
                    e.value.map(str::to_string),
                )
            })
            .collect())
    }

    fn triple(section: &str, name: &str, value: Option<&str>) -> (String, String, Option<String>) {
        (
            section.to_string(),
            name.to_string(),
            value.map(str::to_string),
        )
    }

    #[test]
    fn test_parse_edits() {
        assert_eq!(
            parse(&["a.b=c", "d.e", "f", "g.h.i=j=k", "l.m="], false).unwrap(),
            vec![
                triple("a", "b", Some("c")),
                triple("d", "e", Some("f")),
                triple("g", "h.i", Some("j=k")),
                triple("l", "m", Some("")),
            ]
        );
        assert_eq!(parse(&[], false).unwrap(), vec![]);
    }

    #[test]
    fn test_parse_edits_rejects_invalid_arguments() {
        for (args, invalid) in [
            (&["missing.value"][..], "missing.value"),
            (&["missing=name"][..], "missing"),
            (&["a.b=c", "d.e"][..], "d.e"),
            (&["a.b", "c", "noname"][..], "noname"),
        ] {
            let err = parse(args, false).unwrap_err().to_string();
            assert_eq!(
                err,
                format!("invalid config edit: '{invalid}'\n(try section.name=value)"),
                "{args:?}"
            );
        }
    }

    #[test]
    fn test_parse_deletions() {
        assert_eq!(
            parse(&["a.b", "c.d.e"], true).unwrap(),
            vec![triple("a", "b", None), triple("c", "d.e", None)]
        );
        for (args, invalid) in [(&["a.b=c"][..], "a.b=c"), (&["a.b", "nodot"][..], "nodot")] {
            let err = parse(args, true).unwrap_err().to_string();
            assert_eq!(
                err,
                format!("invalid config deletion: '{invalid}'\n(try section.name)"),
                "{args:?}"
            );
        }
    }
}
