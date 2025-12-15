/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Benchmark for virtual-tree algorithms.

use std::sync::OnceLock;

use dag::Dag;
use minibench::bench;
use minibench::elapsed;
use nonblocking::non_blocking_result as n;
use storemodel::TreeItemFlag;
use types::Id20;
use types::SerializationFormat;
use virtual_repo::VirtualRepoProvider;
use virtual_repo::text_gen::generate_file_content_of_length;
use virtual_repo::text_gen::generate_paragraphs;

const FORMAT: SerializationFormat = SerializationFormat::Hg;

fn main() {
    for (size, n) in [(3000, 10000), (10_000_000, 1)] {
        bench(
            format!("generate_file_content_of_length({size}) {n} times"),
            || {
                elapsed(|| {
                    for _ in 0..n {
                        let _content = generate_file_content_of_length(size);
                    }
                })
            },
        );
    }

    // Warm-up (load and analyse corpus).
    let _ = generate_paragraphs(1, 0);
    for (size, n) in [(3000, 1000), (10_000_000, 1)] {
        bench(format!("generate_paragraphs({size}) {n} times"), || {
            elapsed(|| {
                for _ in 0..n {
                    let _content = generate_paragraphs(size, 42);
                }
            })
        });
    }

    // Scan the first 2-level of trees of
    let provider = VirtualRepoProvider::new(FORMAT);
    manifest_tree::init();
    for (factor_bits, depth) in [(2u8, 4), (4, 2), (8, 2), (12, 2), (20, 1)] {
        let stat_str = OnceLock::new();
        bench(
            format!("visit trees factor_bits={factor_bits} depth={depth}"),
            || {
                let dir = tempfile::tempdir().unwrap();
                let mut dag = Dag::open(dir.path()).unwrap();
                let head_id = n(virtual_repo::populate_dag(&mut dag, factor_bits)).unwrap();
                let head_id20 = Id20::from_slice(head_id.as_ref()).unwrap();
                let commit_body = provider.get_content(head_id20).unwrap();
                let root_tree_id20 =
                    format_util::commit_text_to_root_tree_id(&commit_body, FORMAT).unwrap();
                elapsed(|| {
                    let mut stat = TreeStat::default();
                    visit_tree(&provider, root_tree_id20, depth, &mut stat);
                    stat_str.get_or_init(|| {
                        let avg_size = stat.size / stat.files;
                        format!("  factor_bits={factor_bits} depth={depth} => avg_size={avg_size}\n    {stat:?}")
                    });
                })
            },
        );
        if let Some(stat_str) = stat_str.get() {
            eprintln!("{}", stat_str);
        }
    }
}

#[derive(Debug, Default)]
struct TreeStat {
    size: usize,
    files: usize,
    dirs: usize,
}

/// Recursively read trees and blobs up-to `depth`. `depth=0` means reading nothing.
fn visit_tree(provider: &VirtualRepoProvider, tree_id20: Id20, depth: usize, stat: &mut TreeStat) {
    if depth == 0 {
        return;
    }
    let tree_data = provider.get_content(tree_id20).unwrap();
    for entry in storemodel::basic_parse_tree(tree_data, FORMAT)
        .unwrap()
        .iter()
        .unwrap()
    {
        let (_name, id20, flag) = entry.unwrap();
        match flag {
            TreeItemFlag::File(_file_type) => {
                stat.files += 1;
                let blob = provider.get_content(id20).unwrap();
                stat.size += blob.len();
            }
            TreeItemFlag::Directory => {
                stat.dirs += 1;
                visit_tree(provider, id20, depth - 1, stat);
            }
        }
    }
}

// Supports turning on tracing via LOG=...
dev_logger::init!();
