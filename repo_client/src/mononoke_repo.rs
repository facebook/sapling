// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt::{self, Debug};
use std::path::Path;
use std::time::Duration;

use rand::Isaac64Rng;
use rand::distributions::{Distribution, LogNormal};
use slog::Logger;

use blobrepo::BlobRepo;
use mercurial_types::RepositoryId;
use metaconfig::repoconfig::RepoType;

use errors::*;

struct LogNormalGenerator {
    rng: Isaac64Rng,
    distribution: LogNormal,
}

#[derive(Clone)]
pub struct MononokeRepo {
    log_name: String,
    blobrepo: BlobRepo,
}

impl MononokeRepo {
    #[inline]
    pub fn new(logger: Logger, repo: &RepoType, repoid: RepositoryId) -> Result<Self> {
        // Just use the path as the name.
        let log_name = format!("{}", repo.path().to_owned().display());
        Self::new_with_name(logger, repo, repoid, log_name)
    }

    pub fn new_with_name(
        logger: Logger,
        repo: &RepoType,
        repoid: RepositoryId,
        log_name: impl Into<String>,
    ) -> Result<Self> {
        let log_name = log_name.into();
        Ok(MononokeRepo {
            log_name,
            blobrepo: repo.open(logger, repoid)?,
        })
    }

    #[inline]
    pub fn log_name(&self) -> &str {
        self.log_name.as_ref()
    }

    #[inline]
    pub fn blobrepo(&self) -> &BlobRepo {
        &self.blobrepo
    }
}

impl Debug for MononokeRepo {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Repo({})", self.log_name)
    }
}

trait OpenableRepoType {
    fn open(&self, logger: Logger, repoid: RepositoryId) -> Result<BlobRepo>;
    fn path(&self) -> &Path;
}

impl OpenableRepoType for RepoType {
    fn open(&self, logger: Logger, repoid: RepositoryId) -> Result<BlobRepo> {
        use hgproto::ErrorKind;
        use metaconfig::repoconfig::RepoType::*;

        let ret = match *self {
            Revlog(_) => Err(ErrorKind::CantServeRevlogRepo)?,
            BlobFiles(ref path) => BlobRepo::new_files(logger, &path, repoid)?,
            BlobRocks(ref path) => BlobRepo::new_rocksdb(logger, &path, repoid)?,
            BlobManifold { ref args, .. } => BlobRepo::new_manifold(logger, args, repoid)?,
            TestBlobDelayRocks(ref path, mean, stddev) => {
                // We take in an arithmetic mean and stddev, and deduce a log normal
                let mean = mean as f64 / 1_000_000.0;
                let stddev = stddev as f64 / 1_000_000.0;
                let variance = stddev * stddev;
                let mean_squared = mean * mean;

                let mu = (mean_squared / (variance + mean_squared).sqrt()).ln();
                let sigma = (1.0 + variance / mean_squared).ln();

                let max_delay = 16.0;

                let mut delay_gen = LogNormalGenerator {
                    // This is a deterministic RNG if not seeded
                    rng: Isaac64Rng::new_from_u64(0),
                    distribution: LogNormal::new(mu, sigma),
                };
                let delay_gen = move |()| {
                    let delay = delay_gen.distribution.sample(&mut delay_gen.rng);
                    let delay = if delay < 0.0 || delay > max_delay {
                        max_delay
                    } else {
                        delay
                    };
                    let seconds = delay as u64;
                    let nanos = ((delay - seconds as f64) * 1_000_000_000.0) as u32;
                    Duration::new(seconds, nanos)
                };
                BlobRepo::new_rocksdb_delayed(
                    logger,
                    &path,
                    repoid,
                    delay_gen,
                    // Roundtrips to the server - i.e. how many delays to apply
                    2, // get
                    3, // put
                    2, // is_present
                    2, // assert_present
                )?
            }
        };

        Ok(ret)
    }

    fn path(&self) -> &Path {
        use metaconfig::repoconfig::RepoType::*;

        match *self {
            Revlog(ref path) | BlobFiles(ref path) | BlobRocks(ref path) => path.as_ref(),
            BlobManifold { ref path, .. } => path.as_ref(),
            TestBlobDelayRocks(ref path, ..) => path.as_ref(),
        }
    }
}
