/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use quickcheck::{Arbitrary, Gen};
use rand::Rng;

use crate::{
    dataentry::DataEntry,
    hgid::HgId,
    key::Key,
    parents::Parents,
    path::{PathComponent, PathComponentBuf, RepoPath, RepoPathBuf},
};

pub fn repo_path(s: &str) -> &RepoPath {
    if s == "" {
        panic!(format!(
            "the empty repo path is special, use RepoPath::empty() to build"
        ));
    }
    RepoPath::from_str(s).unwrap()
}

pub fn repo_path_buf(s: &str) -> RepoPathBuf {
    if s == "" {
        panic!(format!(
            "the empty repo path is special, use RepoPathBuf::new() to build"
        ));
    }
    RepoPathBuf::from_string(s.to_owned()).unwrap()
}

pub fn path_component(s: &str) -> &PathComponent {
    PathComponent::from_str(s).unwrap()
}

pub fn path_component_buf(s: &str) -> PathComponentBuf {
    PathComponentBuf::from_string(s.to_owned()).unwrap()
}

pub fn hgid(hex: &str) -> HgId {
    if hex.len() > HgId::hex_len() {
        panic!(format!("invalid length for hex hgid: {}", hex));
    }
    if hex == "0" {
        panic!(format!("hgid 0 is special, use HgId::null_id() to build"));
    }
    let mut buffer = String::new();
    for _i in 0..HgId::hex_len() - hex.len() {
        buffer.push('0');
    }
    buffer.push_str(hex);
    HgId::from_str(&buffer).unwrap()
}

pub fn key(path: &str, hexnode: &str) -> Key {
    Key::new(repo_path_buf(path), hgid(hexnode))
}

/// The null hgid id is special and it's semantics vary. A null key contains a null hgid id.
pub fn null_key(path: &str) -> Key {
    Key::new(repo_path_buf(path), HgId::null_id().clone())
}

pub fn data_entry(key: Key, data: impl AsRef<[u8]>) -> DataEntry {
    DataEntry::new(key, data.as_ref().into(), Parents::None)
}

pub fn generate_repo_paths<G: Gen>(count: usize, qc_gen: &mut G) -> Vec<RepoPathBuf> {
    struct Generator<'a, G: Gen> {
        current_path: RepoPathBuf,
        current_component_length: usize,
        min_files_per_dir: usize,
        directory_component_min: usize,
        directory_component_max: usize,
        generated_paths: Vec<RepoPathBuf>,
        generate_paths_cnt: usize,
        qc_gen: &'a mut G,
    }
    impl<'a, G: Gen> Generator<'a, G> {
        fn generate_directory<'b>(&'b mut self) {
            let dir_components_cnt = if self.current_component_length == 0 {
                std::usize::MAX
            } else {
                self.qc_gen
                    .gen_range(self.directory_component_min, self.directory_component_max)
            };
            let mut component_hash = HashSet::new();
            for i in 0..dir_components_cnt {
                if self.generate_paths_cnt <= self.generated_paths.len() {
                    break;
                }
                let component = PathComponentBuf::arbitrary(self.qc_gen);
                if component_hash.contains(&component) {
                    continue;
                }
                self.current_path.push(component.as_ref());
                component_hash.insert(component);
                self.current_component_length += 1;

                // Decide if this is a directory. As we nest more and more directories, the
                // probabilty of having directories decreses.
                let u = self.current_component_length as u32;
                if i < self.min_files_per_dir || self.qc_gen.gen_ratio(u + 1, u + 2) {
                    self.generated_paths.push(self.current_path.clone());
                } else {
                    self.generate_directory();
                }
                self.current_path.pop();
                self.current_component_length -= 1;
            }
        }
    }

    let mut generator = Generator {
        current_path: RepoPathBuf::new(),
        current_component_length: 0,
        min_files_per_dir: 2,
        directory_component_min: 3,
        directory_component_max: 20,
        generated_paths: Vec::with_capacity(count),
        generate_paths_cnt: count,
        qc_gen,
    };

    generator.generate_directory();
    generator.generated_paths
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::StdGen;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;

    #[test]
    fn test_generate_repo_paths() {
        let rng = ChaChaRng::from_seed([0u8; 32]);
        let mut qc_gen = StdGen::new(rng, 10);
        let count = 10000;
        let paths = generate_repo_paths(count, &mut qc_gen);
        assert_eq!(paths.len(), count);
    }
}
