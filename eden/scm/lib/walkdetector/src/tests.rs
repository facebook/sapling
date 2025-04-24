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
    let detector = Detector::new();

    assert_eq!(detector.walks().len(), 0);

    let epoch = Instant::now();

    detector.file_read(epoch, p("dir/a"))?;
    detector.file_read(epoch, p("dir/a"))?;

    assert_eq!(detector.walks().len(), 0);

    detector.file_read(epoch, p("dir/b"))?;

    // FIXME - should be a walk
    assert_eq!(detector.walks().len(), 0);

    Ok(())
}
