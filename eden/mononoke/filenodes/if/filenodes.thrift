/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

include "eden/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"
include "eden/mononoke/mercurial/types/if/mercurial_thrift.thrift"

/// Code version used in memcache keys.  This should be changed whenever
/// the layout of memcache entries is changed in an incompatible way.
/// The corresponding sitever, which can be used to flush memcache, is
/// in the JustKnob scm/mononoke_memcache_sitevers:filenodes.
const i32 MC_CODEVER = 3;

union FilenodeInfoList {
  1: list<FilenodeInfo> Data;
  2: list<i64> Pointers;
  // This actual value is ignored
  3: byte TooBig;
}

struct FilenodeInfo {
  // 1 was used previously.
  2: required mercurial_thrift.HgNodeHash filenode;
  3: optional mercurial_thrift.HgNodeHash p1;
  4: optional mercurial_thrift.HgNodeHash p2;
  5: optional FilenodeCopyFrom copyfrom;
  6: required mercurial_thrift.HgNodeHash linknode;
} (rust.exhaustive)

struct FilenodeCopyFrom {
  1: required mononoke_types_thrift.RepoPath path;
  2: required mercurial_thrift.HgNodeHash filenode;
} (rust.exhaustive)
