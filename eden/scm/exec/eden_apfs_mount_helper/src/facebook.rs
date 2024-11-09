/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

const MARKER_FILE: &str = "/tmp/apfs_broken";

/// We are seeing issues with APFS mount command on lego-mac and we believe it is
/// a bug in APFS that can only be resolved with restarting the machine. This
/// function writes a marker file on Sandcastle so FBAR can restart the host.
pub fn write_apfs_issue_marker() {
    // std::fs::write overwrites when the file already exists
    std::fs::write(MARKER_FILE, "1").ok();
}
