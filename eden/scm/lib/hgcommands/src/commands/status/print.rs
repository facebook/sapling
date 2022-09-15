/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Result;
use formatter::formatter::Formattable;
use formatter::formatter::ListFormatter;
use serde::Serialize;
use types::path::RepoPathRelativizer;
use types::RepoPath;
use types::RepoPathBuf;

/// Config that determines how the output of `hg status` will be printed to the console.
pub struct PrintConfig {
    /// Determines which types of statuses will be displayed.
    pub status_types: PrintConfigStatusTypes,
    /// If true, the status will not be printed: only the path.
    pub no_status: bool,
    /// If true, for each file that was copied/moved, the source of copy/move will be printed
    /// below the destination.
    pub copies: bool,
    /// Termination character for the line: in practice this is '\0' or '\n'.
    pub endl: char,
    /// If true, paths are printed relative to the root of the repository; otherwise, they are
    /// printed relative to getcwd(2).
    pub root_relative: bool,
    /// Whether ANSI color codes should be used in the output.
    pub use_color: bool,
}

/// This struct covers the possible set of values for `hg status`. Used in conjunction with
/// PrintConfig: paths will only be included in the output of `hg status` if their corresponding
/// status is true in this struct.
pub struct PrintConfigStatusTypes {
    pub modified: bool,
    pub added: bool,
    pub removed: bool,
    pub deleted: bool,
    pub clean: bool,
    pub unknown: bool,
    pub ignored: bool,
}

enum PrintGroup {
    Modified,
    Added,
    Removed,
    Deleted,
    Unknown,
    Ignored,
    Clean,
}

/// Wrapper around an ordinary PathRelativizer that honors the --root-relative flag to `hg status`.
struct HgStatusPathRelativizer {
    relativizer: Option<RepoPathRelativizer>,
}

impl HgStatusPathRelativizer {
    /// * `root_relative` true if --root-relative was specified.
    /// * `relativizer` comes from HgArgs.relativizer.
    pub fn new(root_relative: bool, relativizer: RepoPathRelativizer) -> HgStatusPathRelativizer {
        let relativizer = match (root_relative, relativizer) {
            (false, r) => Some(r),
            _ => None,
        };
        HgStatusPathRelativizer { relativizer }
    }

    /// Returns a String that is suitable for display to the user.
    ///
    /// If `root_relative` is true, the path returned will be relative to the working directory.
    pub fn relativize(&self, repo_path: &RepoPath) -> String {
        let out = match self.relativizer {
            Some(ref relativizer) => relativizer.relativize(repo_path),
            None => repo_path.to_string(),
        };

        if !out.is_empty() {
            out
        } else {
            // In the rare event that the relativized path results in the empty string, print "."
            // instead so the user does not end up with an empty line.
            String::from(".")
        }
    }
}

const RED: &str = "\u{001B}[31m";
const BLUE: &str = "\u{001B}[34m";
const MAGENTA: &str = "\u{001B}[35m";
const GREEN: &str = "\u{001B}[32m";
const CYAN: &str = "\u{001B}[36m";
const BRIGHT_BLACK: &str = "\u{001B}[30;1m"; // Effectively grey.

const BOLD: &str = "\u{001B}[1m";
const UNDERLINE: &str = "\u{001b}[4m";
const RESET: &str = "\u{001B}[0m";

#[derive(Serialize)]
struct StatusEntry<'a> {
    path: String,

    status: &'a str,

    #[serde(skip_serializing_if = "Option::is_none")]
    copy: Option<String>,

    #[serde(skip_serializing)]
    ansi_color_prefix: &'a str,

    #[serde(skip_serializing)]
    print_config: &'a PrintConfig,
}

impl<'a> Formattable for StatusEntry<'a> {
    fn format_plain(
        &self,
        _options: &formatter::formatter::FormatOptions,
        writer: &mut dyn formatter::StyleWrite,
    ) -> Result<(), anyhow::Error> {
        let status = if self.print_config.no_status {
            "".to_owned()
        } else {
            format!("{} ", self.status)
        };
        let (colorized_status, ansi_suffix) = if self.print_config.use_color {
            (
                format!("{}{}", self.ansi_color_prefix, status),
                RESET.to_string(),
            )
        } else {
            (status.to_owned(), "".to_owned())
        };
        write!(
            writer,
            "{}{}{}{}",
            colorized_status, self.path, ansi_suffix, self.print_config.endl
        )?;
        if let Some(p) = &self.copy {
            write!(writer, "  {}{}", p, self.print_config.endl)?;
        }
        Ok(())
    }
}

pub fn print_status(
    mut formatter: Box<dyn ListFormatter>,
    relativizer: RepoPathRelativizer,
    print_config: &PrintConfig,
    status: &status::Status,
    copymap: &HashMap<RepoPathBuf, RepoPathBuf>,
) -> Result<()> {
    formatter.begin_list()?;

    let relativizer = HgStatusPathRelativizer::new(print_config.root_relative, relativizer);
    let mut print_group =
        |print_group, enabled: bool, group: &mut dyn Iterator<Item = &RepoPathBuf>| -> Result<()> {
            if !enabled {
                return Ok(());
            }

            // `hg config | grep color` did not yield the entries for color.status listed on
            // https://www.mercurial-scm.org/wiki/ColorExtension. At Meta, we seem to match
            // the defaults listed on the wiki page, except we don't change the background color.
            let (status, ansi_color_prefix) = match print_group {
                PrintGroup::Modified => ("M", format!("{}{}", BLUE, BOLD)),
                PrintGroup::Added => ("A", format!("{}{}", GREEN, BOLD)),
                PrintGroup::Removed => ("R", format!("{}{}", RED, BOLD)),
                PrintGroup::Deleted => ("!", format!("{}{}{}", CYAN, BOLD, UNDERLINE)),
                PrintGroup::Unknown => ("?", format!("{}{}{}", MAGENTA, BOLD, UNDERLINE)),
                PrintGroup::Ignored => ("I", format!("{}{}", BRIGHT_BLACK, BOLD)),
                PrintGroup::Clean => ("C", "".to_owned()),
            };

            let mut group = group.collect::<Vec<_>>();
            group.sort();
            for path in group {
                formatter.format_item(&StatusEntry {
                    path: relativizer.relativize(path),
                    status,
                    copy: copymap.get(path).map(|p| relativizer.relativize(p)),
                    ansi_color_prefix: ansi_color_prefix.as_str(),
                    print_config,
                })?;
            }
            Ok(())
        };

    print_group(
        PrintGroup::Modified,
        print_config.status_types.modified,
        &mut status.modified(),
    )?;
    print_group(
        PrintGroup::Added,
        print_config.status_types.added,
        &mut status.added(),
    )?;
    print_group(
        PrintGroup::Removed,
        print_config.status_types.removed,
        &mut status.removed(),
    )?;
    print_group(
        PrintGroup::Deleted,
        print_config.status_types.deleted,
        &mut status.deleted(),
    )?;
    print_group(
        PrintGroup::Unknown,
        print_config.status_types.unknown,
        &mut status.unknown(),
    )?;
    print_group(
        PrintGroup::Ignored,
        print_config.status_types.ignored,
        &mut status.ignored(),
    )?;
    print_group(
        PrintGroup::Clean,
        print_config.status_types.clean,
        &mut status.clean(),
    )?;

    formatter.end_list()?;

    Ok(())
}

#[cfg(test)]
impl Default for PrintConfig {
    fn default() -> Self {
        PrintConfig {
            status_types: PrintConfigStatusTypes {
                modified: true,
                added: true,
                removed: true,
                deleted: true,
                clean: false,
                unknown: true,
                ignored: false,
            },
            no_status: false,
            copies: false,
            endl: '\n',
            root_relative: false,
            use_color: false,
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;
    use std::str;

    use clidispatch::io::IO;
    use formatter::formatter::get_formatter;
    use formatter::formatter::FormatOptions;

    use super::*;

    fn repo_path_buf(path: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(path.to_string()).unwrap()
    }

    fn extract_output(io: IO) -> (String, String) {
        let stdout = io.with_output(|o| o.as_any().downcast_ref::<Vec<u8>>().unwrap().clone());
        let stdout = str::from_utf8(&stdout).unwrap().to_string();
        let stderr = io.with_error(|e| {
            e.as_ref()
                .unwrap()
                .as_any()
                .downcast_ref::<Vec<u8>>()
                .unwrap()
                .clone()
        });
        let stderr = str::from_utf8(&stderr).unwrap().to_string();
        (stdout, stderr)
    }

    #[derive(Default)]
    struct PrintTestCase {
        print_config: PrintConfig,
        status: status::Status,
        copymap: HashMap<RepoPathBuf, RepoPathBuf>,
        stdout: String,
        stderr: String,
    }

    /// Helper function for testing `print_status`.
    fn test_print(test_case: PrintTestCase) {
        let relativizer = RepoPathRelativizer::new("/repo", "/repo");
        let tin = "".as_bytes();
        let tout = Vec::new();
        let terr = Vec::new();
        let io = IO::new(tin, tout, Some(terr));
        let options = FormatOptions {
            debug: false,
            verbose: false,
            quiet: false,
        };
        let fm = get_formatter(
            &BTreeMap::<&str, &str>::new(),
            "status",
            "",
            options,
            Box::new(io.output()),
        )
        .unwrap();
        print_status(
            fm,
            relativizer,
            &test_case.print_config,
            &test_case.status,
            &test_case.copymap,
        )
        .unwrap();
        let (actual_output, actual_error) = extract_output(io);
        assert_eq!(actual_output, test_case.stdout);
        assert_eq!(actual_error, test_case.stderr);
    }

    // XXX: PathRelativizer is problematic on OSX.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_empty() {
        let status = status::StatusBuilder::new().build();

        test_print(PrintTestCase {
            status,
            ..Default::default()
        });
    }

    // XXX: PathRelativizer is problematic on OSX.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_print_status() {
        let status = status::StatusBuilder::new()
            .modified(vec![repo_path_buf("modified.txt")])
            .added(vec![
                repo_path_buf("added.txt"),
                repo_path_buf("added_even_though_normally_ignored.txt"),
                repo_path_buf("added_other_parent.txt"),
            ])
            .removed(vec![
                repo_path_buf("modified_and_marked_for_removal.txt"),
                repo_path_buf("removed.txt"),
            ])
            .deleted(vec![repo_path_buf(
                "removed_but_not_marked_for_removal.txt",
            )])
            .unknown(vec![repo_path_buf("unknown.txt")])
            .ignored(vec![repo_path_buf("ignored.txt")])
            .build();

        // We have to slice [1..] to strip the leading newline.
        let no_arg_stdout = r#"
M modified.txt
A added.txt
A added_even_though_normally_ignored.txt
A added_other_parent.txt
R modified_and_marked_for_removal.txt
R removed.txt
! removed_but_not_marked_for_removal.txt
? unknown.txt
"#[1..]
            .to_string();
        test_print(PrintTestCase {
            status: status.clone(),
            stdout: no_arg_stdout,
            ..Default::default()
        });

        // We have to slice [1..] to strip the leading newline.
        let mardui_stdout = r#"
M modified.txt
A added.txt
A added_even_though_normally_ignored.txt
A added_other_parent.txt
R modified_and_marked_for_removal.txt
R removed.txt
! removed_but_not_marked_for_removal.txt
? unknown.txt
I ignored.txt
"#[1..]
            .to_string();
        let mut print_config = PrintConfig::default();
        print_config.status_types.ignored = true;
        test_print(PrintTestCase {
            status: status.clone(),
            print_config,
            stdout: mardui_stdout,
            ..Default::default()
        });

        let mardui_color_stdout = concat!(
            "\u{001B}[34m\u{001B}[1mM modified.txt\u{001B}[0m\n",
            "\u{001B}[32m\u{001B}[1mA added.txt\u{001B}[0m\n",
            "\u{001B}[32m\u{001B}[1mA added_even_though_normally_ignored.txt\u{001B}[0m\n",
            "\u{001B}[32m\u{001B}[1mA added_other_parent.txt\u{001B}[0m\n",
            "\u{001B}[31m\u{001B}[1mR modified_and_marked_for_removal.txt\u{001B}[0m\n",
            "\u{001B}[31m\u{001B}[1mR removed.txt\u{001B}[0m\n",
            "\u{001B}[36m\u{001B}[1m\u{001b}[4m! removed_but_not_marked_for_removal.txt\u{001B}[0m\n",
            "\u{001B}[35m\u{001B}[1m\u{001b}[4m? unknown.txt\u{001B}[0m\n",
            "\u{001B}[30;1m\u{001B}[1mI ignored.txt\u{001B}[0m\n",
        );
        let mut print_config = PrintConfig::default();
        print_config.status_types.ignored = true;
        print_config.use_color = true;
        test_print(PrintTestCase {
            status,
            print_config,
            stdout: mardui_color_stdout.to_string(),
            ..Default::default()
        });
    }

    // XXX: PathRelativizer is problematic on OSX.
    #[cfg(target_os = "linux")]
    #[test]
    fn no_status_flag() {
        let status = status::StatusBuilder::new()
            .modified(vec![repo_path_buf("file.txt")])
            .build();

        let print_config = PrintConfig {
            no_status: true,
            ..PrintConfig::default()
        };

        test_print(PrintTestCase {
            status,
            print_config,
            stdout: "file.txt\n".to_owned(),
            ..Default::default()
        });
    }
}
