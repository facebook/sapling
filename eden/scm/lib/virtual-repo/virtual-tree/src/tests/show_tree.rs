/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;

use crate::types::*;

fn count_files(provider: &dyn VirtualTreeProvider, tree_id: TreeId) -> usize {
    let mut file_count = 0;
    for (_name_id, content_id) in provider.read_tree(tree_id) {
        let typed_content_id = TypedContentId::from(content_id);
        match typed_content_id {
            TypedContentId::File(_blob_id, _file_mode) => file_count += 1,
            _ => {}
        }
    }
    file_count
}

fn debug_tree_with_indent(
    provider: &dyn VirtualTreeProvider,
    tree_id: TreeId,
    indent: usize,
    col: usize,
    depth: usize,
    abbrev_files: bool,
    out: &mut dyn fmt::Write,
) -> fmt::Result {
    for (name_id, content_id) in provider.read_tree(tree_id) {
        let name = name_id.0.to_string();
        match TypedContentId::from(content_id) {
            TypedContentId::Tree(subtree_id) => {
                let seed = provider.get_tree_seed(subtree_id);
                write!(
                    out,
                    "{:width1$}{}/{:width2$}#{:<2} seed={}",
                    "",
                    name,
                    "",
                    subtree_id.0,
                    seed.0,
                    width1 = indent * 2,
                    width2 = (col.saturating_sub((indent * 2).saturating_sub(name.len())))
                )?;
                if abbrev_files {
                    let file_count = count_files(provider, subtree_id);
                    write!(out, " files={}", file_count)?;
                }
                out.write_char('\n')?;
                if depth > 0 {
                    debug_tree_with_indent(
                        provider,
                        subtree_id,
                        indent + 1,
                        col,
                        depth - 1,
                        abbrev_files,
                        out,
                    )?
                }
            }
            TypedContentId::File(blob_id, file_mode) => {
                if abbrev_files {
                    continue;
                }
                let mode = match file_mode {
                    FileMode::Regular => "",
                    FileMode::Executable => "x",
                    FileMode::Symlink => "l",
                };
                write!(
                    out,
                    "{:width$}{} = {}{}\n",
                    "",
                    name,
                    blob_id.0,
                    mode,
                    width = indent * 2
                )?;
            }
            TypedContentId::Absent => {
                write!(out, "{:width$}{} = A\n", "", name, width = indent * 2)?;
            }
        }
    }
    Ok(())
}

pub trait ShowTree {
    /// Show a tree as string.
    fn show_tree(&self, tree_id: TreeId, abbrev_files: bool, depth: usize) -> String {
        let provider = self.as_provider();
        let mut out = "\n".to_string();
        debug_tree_with_indent(provider, tree_id, 0, 30, depth, abbrev_files, &mut out).unwrap();
        out
    }

    /// Show all root trees as a string.
    fn show_root_trees(&self) -> String {
        let provider = self.as_provider();
        let mut out = "\n".to_string();
        for i in 0..provider.root_tree_len() {
            let tree_id = provider.root_tree_id(i);
            let seed = provider.get_tree_seed(tree_id);
            write!(
                &mut out as &mut dyn fmt::Write,
                "            Root tree {}:         #{:<2} seed={}\n",
                i + 1,
                tree_id.0,
                seed.0
            )
            .unwrap();
            debug_tree_with_indent(provider, tree_id, 7, 30, usize::MAX, false, &mut out).unwrap();
        }
        // Remove the trailing new line.
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }

    /// Implementations should fill this method.
    fn as_provider(&self) -> &dyn VirtualTreeProvider;
}

impl ShowTree for dyn VirtualTreeProvider {
    fn as_provider(&self) -> &dyn VirtualTreeProvider {
        self
    }
}

impl<T: VirtualTreeProvider> ShowTree for T {
    fn as_provider(&self) -> &dyn VirtualTreeProvider {
        self
    }
}
