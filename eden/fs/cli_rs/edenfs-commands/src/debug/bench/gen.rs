/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Test data structures for benchmarking

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use blake3::Hash;
use rand::Rng as _;

use super::types;

/// TestDir represents a directory used for testing.
/// It handles creation, validation, and removal of test directories,
/// as well as generating paths for test files and databases.
pub struct TestDir {
    // Path to the test directory
    pub path: PathBuf,
}

impl TestDir {
    /// Validates and prepares a test directory.
    /// Returns a TestDir instance if successful.
    pub fn validate(test_dir: &str) -> Result<Self> {
        let test_dir_path = Path::new(test_dir);
        fs::create_dir_all(test_dir_path)
            .with_context(|| format!("failed to create test directory {test_dir}"))?;
        let bench_dir_path = test_dir_path.join(types::BENCH_DIR_NAME);
        if bench_dir_path.exists() {
            fs::remove_dir_all(&bench_dir_path)?;
        }
        fs::create_dir(&bench_dir_path)?;
        Ok(TestDir {
            path: bench_dir_path,
        })
    }

    /// Prepares the hash-prefix directories used by the database benchmarks.
    pub(crate) fn prepare_hash_directories(&self) -> Result<()> {
        for i in 0..types::NUMBER_OF_SUB_DIRS {
            let sub_dir = format!("{i:02x}");
            let sub_dir_path = self.path.join(sub_dir);
            fs::create_dir_all(&sub_dir_path)?;
        }
        Ok(())
    }

    /// Removes the test directory.
    pub fn remove(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_dir_all(&self.path)?;
        }
        Ok(())
    }

    /// Returns the path to the LMDB file.
    pub fn lmdb_path(&self) -> PathBuf {
        self.path.join(types::LMDB_FILE_NAME)
    }

    /// Returns the path to the SQLite file.
    pub fn sqlite_path(&self) -> PathBuf {
        self.path.join(types::SQLITE_FILE_NAME)
    }
}

pub struct RandomData {
    // Number of randomly generated files.
    pub number_of_files: usize,

    // Size of each chunk in bytes.
    pub chunk_size: usize,

    // Random content that will be written to files.
    pub chunks: Vec<Vec<u8>>,

    // Hashes to verify the data written to files.
    // Also used for generate file paths contents will be written to.
    pub hashes: Vec<Hash>,
}

impl RandomData {
    pub fn new(number_of_files: usize, chunk_size: usize) -> Self {
        let mut rng = rand::rng();
        let mut chunks = Vec::with_capacity(number_of_files);
        let mut hashes = Vec::with_capacity(number_of_files);
        for _ in 0..number_of_files {
            let mut chunk = vec![0u8; chunk_size];
            rng.fill_bytes(&mut chunk);
            let hash = blake3::hash(&chunk);
            chunks.push(chunk);
            hashes.push(hash);
        }
        RandomData {
            number_of_files,
            chunk_size,
            chunks,
            hashes,
        }
    }

    pub fn keys(&self) -> Vec<Vec<u8>> {
        self.hashes.iter().map(|h| h.as_bytes().to_vec()).collect()
    }

    pub fn total_size(&self) -> usize {
        self.number_of_files * self.chunk_size
    }
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn creates_missing_test_directory() {
        let parent = TempDir::new().expect("temporary directory should be created");
        let test_dir_path = parent.path().join("missing").join("nested");

        let test_dir = TestDir::validate(
            test_dir_path
                .to_str()
                .expect("temporary path should be UTF-8"),
        )
        .expect("missing test directory should be created");

        assert!(test_dir_path.is_dir());
        assert_eq!(test_dir.path, test_dir_path.join(types::BENCH_DIR_NAME));
        test_dir
            .remove()
            .expect("benchmark directory should be removed");
        assert!(
            test_dir_path.is_dir(),
            "the requested directory should remain"
        );
    }
}
