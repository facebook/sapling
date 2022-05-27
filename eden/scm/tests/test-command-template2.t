#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ newrepo

# Test ifgt function

  $ hg log -T '{ifgt(2, 1, "GT", "NOTGT")} {ifgt(2, 2, "GT", "NOTGT")} {ifgt(2, 3, "GT", "NOTGT")}\n' -r null
  GT NOTGT NOTGT

  $ hg log -T '{ifgt("2", "1", "GT", "NOTGT")} {ifgt("2", "2", "GT", "NOTGT")} {ifgt("2", 3, "GT", "NOTGT")}\n' -r null
  GT NOTGT NOTGT
