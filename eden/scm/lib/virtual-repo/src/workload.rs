/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use anyhow::bail;
use virtual_tree::types::TreeId;
use virtual_tree::types::TypedContentId;
use virtual_tree::types::VirtualTreeProvider;

use crate::MAX_FACTOR_BITS;
use crate::provider::calculate_file_length;
use crate::provider::get_tree_provider;
use crate::text_gen;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedFile {
    pub path: String,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedWorkload {
    pub factor_bits: u8,
    pub files: Vec<GeneratedFile>,
}

/// Generate files from the head tree of the smallest Virtual Repo size factor
/// containing at least `number_of_files` files.
pub fn generate_workload(number_of_files: usize) -> Result<GeneratedWorkload> {
    if number_of_files == 0 {
        return Ok(GeneratedWorkload {
            factor_bits: 0,
            files: Vec::new(),
        });
    }

    for factor_bits in 0..=MAX_FACTOR_BITS as u8 {
        let provider = get_tree_provider(factor_bits);
        let root_tree = head_tree(provider.as_ref())?;
        let mut files = Vec::with_capacity(number_of_files);
        collect_files(
            provider.as_ref(),
            root_tree,
            "",
            number_of_files,
            &mut files,
        );
        if files.len() == number_of_files {
            return Ok(GeneratedWorkload { factor_bits, files });
        }
    }
    bail!("Virtual Repo cannot generate {number_of_files} files")
}

/// Root trees are indexed in commit order, so the last one belongs to the
/// newest commit (`virtual/main`).
fn head_tree(provider: &dyn VirtualTreeProvider) -> Result<TreeId> {
    let Some(index) = provider.root_tree_len().checked_sub(1) else {
        bail!("Virtual Repo has no root trees");
    };
    Ok(provider.root_tree_id(index))
}

fn collect_files(
    provider: &dyn VirtualTreeProvider,
    tree_id: TreeId,
    parent_path: &str,
    limit: usize,
    files: &mut Vec<GeneratedFile>,
) {
    if files.len() == limit {
        return;
    }

    let seed = provider.get_tree_seed(tree_id).0;
    for (name_id, content_id) in provider.read_tree(tree_id) {
        let name_id = name_id.0.get();
        let name = text_gen::generate_file_name(name_id, seed);
        let path = join_path(parent_path, &name);
        match TypedContentId::from(content_id) {
            TypedContentId::Tree(child_tree) => {
                collect_files(provider, child_tree, &path, limit, files);
            }
            TypedContentId::File(blob_id, _mode) => {
                files.push(GeneratedFile {
                    path,
                    size: calculate_file_length(seed, name_id, blob_id.0.get()),
                });
            }
            TypedContentId::Absent => {}
        }
        if files.len() == limit {
            return;
        }
    }
}

fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn generates_requested_files() {
        let workload = generate_workload(1_000).expect("workload generation should succeed");

        assert_eq!(workload.files.len(), 1_000);
        assert_eq!(
            workload
                .files
                .iter()
                .map(|file| &file.path)
                .collect::<HashSet<_>>()
                .len(),
            1_000
        );
        assert!(workload.files.iter().any(|file| file.path.contains('/')));
    }

    #[test]
    fn empty_workload_uses_base_factor() {
        assert_eq!(
            generate_workload(0).expect("empty workload should succeed"),
            GeneratedWorkload {
                factor_bits: 0,
                files: Vec::new(),
            }
        );
    }
}
