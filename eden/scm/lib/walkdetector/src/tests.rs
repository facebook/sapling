/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::time::Instant;

use anyhow::Result;
use types::RepoPathBuf;

use crate::Detector;

fn p(p: impl AsRef<str>) -> RepoPathBuf {
    p.as_ref().to_string().try_into().unwrap()
}

#[test]
fn test_walk_big_dir() -> Result<()> {
    let mut detector = Detector::new();
    detector.set_min_dir_walk_threshold(2);

    assert_eq!(detector.walks().len(), 0);

    let epoch = Instant::now();

    detector.file_read(epoch, p("dir/a"))?;
    detector.file_read(epoch, p("dir/a"))?;

    assert_eq!(detector.walks().len(), 0);

    detector.file_read(epoch, p("dir/b"))?;

    assert_eq!(detector.walks(), vec![(p("dir"), 0)]);

    detector.file_read(epoch, p("dir/c"))?;
    detector.file_read(epoch, p("dir/d"))?;
    detector.file_read(epoch, p("dir/e"))?;

    assert_eq!(detector.walks(), vec![(p("dir"), 0)]);

    Ok(())
}

#[test]
fn test_bfs_walk() -> Result<()> {
    let mut detector = Detector::new();
    detector.set_min_dir_walk_threshold(2);

    let epoch = Instant::now();

    detector.file_read(epoch, p("root/dir1/a"))?;
    detector.file_read(epoch, p("root/dir1/b"))?;

    assert_eq!(detector.walks(), vec![(p("root/dir1"), 0)]);

    detector.file_read(epoch, p("root/dir2/a"))?;
    detector.file_read(epoch, p("root/dir2/b"))?;

    // Raised walk up to parent directory with depth=1.
    assert_eq!(detector.walks(), vec![(p("root"), 1)]);

    // Now walk proceeds to the next level.

    detector.file_read(epoch, p("root/dir1/dir1_1/a"))?;
    detector.file_read(epoch, p("root/dir1/dir1_1/b"))?;

    // Nothing combined yet.
    assert_eq!(
        detector.walks(),
        vec![(p("root"), 1), (p("root/dir1/dir1_1"), 0)]
    );

    detector.file_read(epoch, p("root/dir1/dir1_2/a"))?;
    detector.file_read(epoch, p("root/dir1/dir1_2/b"))?;

    // Now we get a second walk for root/dir1
    assert_eq!(detector.walks(), vec![(p("root"), 1), (p("root/dir1"), 1)]);

    // More reads in dir2 doesn't combine the walks
    detector.file_read(epoch, p("root/dir2/c"))?;
    detector.file_read(epoch, p("root/dir2/d"))?;
    assert_eq!(detector.walks(), vec![(p("root"), 1), (p("root/dir1"), 1)]);

    // Walking further in root/dir2 will combine up to root.
    detector.file_read(epoch, p("root/dir2/dir2_1/a"))?;
    detector.file_read(epoch, p("root/dir2/dir2_1/b"))?;
    detector.file_read(epoch, p("root/dir2/dir2_2/a"))?;
    detector.file_read(epoch, p("root/dir2/dir2_2/b"))?;
    assert_eq!(detector.walks(), vec![(p("root"), 2)]);

    Ok(())
}
