/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Cursor;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::OnceLock;

use vlqencoding::VLQDecode;

use crate::types::*;

/// Example trees stored in a compressed buffer.
/// Use `create_example.py` to produce such compressed buffer.
#[derive(Clone)]
pub struct SerializedTree {
    compressed_buffer: &'static [u8],
    buffer: Arc<OnceLock<Vec<u8>>>,
    metadata: Arc<OnceLock<Metadata>>,
}

#[derive(Clone)]
struct Metadata {
    root_tree_len: usize,
    tree_len: usize,
    tree_offsets: Vec<usize>,
}

impl SerializedTree {
    pub fn new(compressed_buffer: &'static [u8]) -> Self {
        Self {
            compressed_buffer,
            buffer: Arc::new(OnceLock::new()),
            metadata: Arc::new(OnceLock::new()),
        }
    }

    fn buffer(&self) -> &[u8] {
        self.buffer
            .get_or_init(|| zstdelta::apply(b"", self.compressed_buffer).unwrap())
    }

    fn metadata(&self) -> &Metadata {
        self.metadata.get_or_init(|| {
            let mut cursor = Cursor::new(self.buffer());
            let tree_len: usize = cursor.read_vlq().unwrap();
            let root_tree_len: usize = cursor.read_vlq().unwrap();
            let mut tree_offsets = Vec::with_capacity(tree_len + 1);

            let mut tree_buffer_offset = 0;
            for _i in 1..=tree_len {
                let len: usize = cursor.read_vlq().unwrap();
                tree_offsets.push(tree_buffer_offset);
                tree_buffer_offset += len;
            }
            tree_offsets.push(tree_buffer_offset);
            let tree_buffer_start = cursor.position() as usize;
            for offset in tree_offsets.iter_mut() {
                *offset += tree_buffer_start;
            }
            Metadata {
                root_tree_len,
                tree_len,
                tree_offsets,
            }
        })
    }

    fn tree_offsets(&self) -> &[usize] {
        &self.metadata().tree_offsets
    }

    fn tree_buffer(&self, index: usize) -> &[u8] {
        assert!(index < self.metadata().tree_len);
        let tree_offsets = self.tree_offsets();
        let tree_start = tree_offsets[index];
        let tree_end = tree_offsets[index + 1];
        &self.buffer()[tree_start..tree_end]
    }
}

impl VirtualTreeProvider for SerializedTree {
    fn read_tree<'a>(&'a self, tree_id: TreeId) -> ReadTreeIter<'a> {
        let tree_index = (tree_id.0.get() - 1) as usize;
        let tree_buf = self.tree_buffer(tree_index);
        let mut cursor = Cursor::new(tree_buf);
        let _seed: u64 = cursor.read_vlq().unwrap();
        let mut name_id = 0;
        let iter = std::iter::from_fn(move || {
            loop {
                name_id += 1;
                let v: Result<u64, _> = cursor.read_vlq();
                match v {
                    Ok(v) => {
                        if v == 0 {
                            continue;
                        }
                        let name_id = NameId(NonZeroU64::new(name_id).unwrap());
                        let (data, flag) = (v >> 2, v & 3);
                        let typed_content_id = match flag {
                            0 => TypedContentId::Tree(TreeId(NonZeroU64::new(data).unwrap())),
                            _ => {
                                let mode = match flag {
                                    2 => FileMode::Executable,
                                    3 => FileMode::Symlink,
                                    _ => FileMode::Regular,
                                };
                                TypedContentId::File(BlobId(NonZeroU64::new(data).unwrap()), mode)
                            }
                        };
                        let content_id = ContentId::from(typed_content_id);
                        return Some((name_id, content_id));
                    }
                    Err(_) => return None,
                }
            }
        });
        Box::new(iter)
    }

    fn get_tree_seed(&self, tree_id: TreeId) -> TreeSeed {
        let tree_index = (tree_id.0.get() - 1) as usize;
        let tree_buf = self.tree_buffer(tree_index);
        let mut cursor = Cursor::new(tree_buf);
        TreeSeed(cursor.read_vlq().unwrap())
    }

    fn root_tree_len(&self) -> usize {
        self.metadata().root_tree_len
    }

    fn root_tree_id(&self, index: usize) -> TreeId {
        TreeId(NonZeroU64::new((index + 1) as u64).unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::*;

    #[test]
    fn test_serialized_example() {
        // Manually created, similar to the test case in TestTree.
        let buf = b"(\xb5/\xfd$,M\x01\x002\xc2\x07\r`Y\x07\x88\xaaOT\xa9z9\x92m\x02-\x187\xc6\x8a\x06\x95Z\xd0\x8f\xe1:Tn\xce\x9f\x04\x02\x00\x00\x14\x07g\x04\x03\xd8\x99\x80";
        let example = SerializedTree::new(buf);
        assert_eq!(
            example.show_root_trees(),
            r#"
            Root tree 1:         #1  seed=1
              1/                 #4  seed=2
                1/               #5  seed=3
                  1 = 1
                2/               #6  seed=4
                  1 = 1
              2/                 #7  seed=5
                1/               #8  seed=6
                  1 = 1
                2/               #9  seed=7
                  1 = 1
            Root tree 2:         #2  seed=1
              1/                 #10 seed=2
                1/               #11 seed=3
                  1 = 2
                2/               #12 seed=4
                  1 = 1
                  2 = 1
              2/                 #7  seed=5
                1/               #8  seed=6
                  1 = 1
                2/               #9  seed=7
                  1 = 1
            Root tree 3:         #3  seed=1
              1/                 #10 seed=2
                1/               #11 seed=3
                  1 = 2
                2/               #12 seed=4
                  1 = 1
                  2 = 1"#
        );
    }
}
