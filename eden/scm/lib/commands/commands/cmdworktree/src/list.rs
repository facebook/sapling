/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use clidispatch::ReqCtx;
use cmdutil::Result;
use cmdutil::get_formatter;
use formatter::FormatOptions;
use formatter::Formattable;
use formatter::StyleWrite;
use fs_err as fs;
use repo::repo::Repo;
use serde::Serialize;
use workingcopy::workingcopy::WorkingCopy;
use worktree::dissolve_group;
use worktree::with_registry_lock;

use crate::WorktreeOpts;

#[derive(Serialize)]
struct ListOutputEntry {
    path: PathBuf,
    role: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    current: bool,
}

impl Formattable for ListOutputEntry {
    fn format_plain(
        &self,
        _options: &FormatOptions,
        writer: &mut dyn StyleWrite,
    ) -> Result<(), anyhow::Error> {
        let marker = if self.current { "*" } else { " " };
        write!(
            writer,
            "{} {:<6}  {}",
            marker,
            self.role,
            self.path.display()
        )?;
        if let Some(label) = &self.label {
            write!(writer, "   {label}")?;
        }
        writeln!(writer)?;
        Ok(())
    }
}

pub(crate) fn run(ctx: &ReqCtx<WorktreeOpts>, repo: &Repo, _wc: &WorkingCopy) -> Result<u8> {
    let mut formatter = get_formatter(
        repo.config(),
        "worktree",
        &ctx.opts.formatter_opts.template,
        ctx.global_opts(),
        Box::new(ctx.io().output()),
    )?;
    let output_empty = |formatter: &mut Box<dyn formatter::ListFormatter>| -> Result<()> {
        if formatter.is_plain() {
            ctx.io().write("this worktree is not part of a group\n")?;
        } else {
            formatter.begin_list()?;
            formatter.end_list()?;
        }
        Ok(())
    };

    let shared_store_path = repo.store_path();
    let current = util::path::strip_unc_prefix(fs::canonicalize(repo.path())?);

    let entries = with_registry_lock(shared_store_path, |registry| {
        let Some(group_id) = registry.find_group_for_path(&current) else {
            return Ok(None);
        };

        let group = registry
            .groups
            .get(&group_id)
            .expect("group must exist after find_group_for_path");

        if !group.main.exists() {
            dissolve_group(registry, &group_id);
            return Ok(None);
        }

        let has_missing = group.worktrees.keys().any(|p| !p.exists());
        if has_missing {
            let group = registry
                .groups
                .get_mut(&group_id)
                .expect("group must exist: not dissolved when main is present");
            group.worktrees.retain(|path, _| path.exists());
            let linked_count = group.worktrees.keys().filter(|p| **p != group.main).count();
            if linked_count == 0 {
                dissolve_group(registry, &group_id);
                return Ok(None);
            }
        }

        let group = registry
            .groups
            .get(&group_id)
            .expect("group must exist: not dissolved when linked worktrees remain");
        let entries: Vec<ListOutputEntry> = group
            .worktrees
            .iter()
            .map(|(path, entry)| {
                let role = if *path == group.main {
                    "main"
                } else {
                    "linked"
                };
                ListOutputEntry {
                    path: path.clone(),
                    role,
                    label: entry.label.clone(),
                    current: *path == current,
                }
            })
            .collect();

        Ok(Some(entries))
    })?;

    match entries {
        None => output_empty(&mut formatter)?,
        Some(entries) => {
            formatter.begin_list()?;
            for entry in &entries {
                formatter.format_item(entry)?;
            }
            formatter.end_list()?;
        }
    }

    Ok(0)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    struct MockWriter(Vec<u8>);

    impl std::io::Write for MockWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.write(buf)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl formatter::StyleWrite for MockWriter {
        fn write_styled(&mut self, _style: &str, text: &str) -> anyhow::Result<()> {
            self.0.write_all(text.as_bytes())?;
            Ok(())
        }
    }

    fn mock_output(writer: &MockWriter) -> String {
        String::from_utf8(writer.0.clone()).unwrap()
    }

    #[test]
    fn test_format_plain_main() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/main"),
            role: "main",
            label: None,
            current: false,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "  main    /repos/main\n");
    }

    #[test]
    fn test_format_plain_linked() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/linked"),
            role: "linked",
            label: None,
            current: false,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "  linked  /repos/linked\n");
    }

    #[test]
    fn test_format_plain_with_label() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/main"),
            role: "main",
            label: Some("my-label".to_string()),
            current: false,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "  main    /repos/main   my-label\n");
    }

    #[test]
    fn test_format_plain_current() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/main"),
            role: "main",
            label: None,
            current: true,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "* main    /repos/main\n");
    }

    #[test]
    fn test_format_plain_current_with_label() {
        let entry = ListOutputEntry {
            path: PathBuf::from("/repos/linked"),
            role: "linked",
            label: Some("dev".to_string()),
            current: true,
        };
        let mut w = MockWriter(Vec::new());
        entry
            .format_plain(&FormatOptions::default(), &mut w)
            .unwrap();
        assert_eq!(mock_output(&w), "* linked  /repos/linked   dev\n");
    }
}
