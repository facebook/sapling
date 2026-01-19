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

use anyhow::Result;
use anyhow::anyhow;
use blake3::Hash;
use rand::RngCore;

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
        if !test_dir_path.exists() {
            return Err(anyhow!("The directory {} does not exist.", test_dir));
        }
        let bench_dir_path = test_dir_path.join(types::BENCH_DIR_NAME);
        if bench_dir_path.exists() {
            fs::remove_dir_all(&bench_dir_path)?;
        }
        fs::create_dir(&bench_dir_path)?;
        Self::prepare_directories(&bench_dir_path)?;
        Ok(TestDir {
            path: bench_dir_path,
        })
    }

    /// Prepares subdirectories for the test directory.
    fn prepare_directories(root: &Path) -> Result<()> {
        for i in 0..types::NUMBER_OF_SUB_DIRS {
            let sub_dir = format!("{:02x}", i);
            let sub_dir_path = root.join(sub_dir);
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

    /// Converts a hash to a file path within the test directory.
    pub fn hash_to_path(&self, hash: &Hash) -> PathBuf {
        let hash_str = hash.to_hex().to_string();
        let sub_dir = &hash_str[0..2];
        self.path.join(sub_dir).join(hash_str)
    }

    /// Returns the path to the combined data file.
    pub fn combined_data_path(&self) -> PathBuf {
        self.path.join(types::COMBINED_DATA_FILE_NAME)
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

    pub fn paths(&self, test_dir: &TestDir) -> Vec<PathBuf> {
        self.hashes
            .iter()
            .map(|hash| test_dir.hash_to_path(hash))
            .collect()
    }

    pub fn keys(&self) -> Vec<Vec<u8>> {
        self.hashes.iter().map(|h| h.as_bytes().to_vec()).collect()
    }

    pub fn total_size(&self) -> usize {
        self.number_of_files * self.chunk_size
    }
}
