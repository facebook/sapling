/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

include "scm/mononoke/mononoke_types/if/mononoke_types_thrift.thrift"
include "scm/mononoke/mercurial/types/if/mercurial_thrift.thrift"

# Memcache constants. Should be change when we want to invalidate memcache
# entries
const i32 MC_CODEVER = 2
const i32 MC_SITEVER = 0

union FilenodeInfoList {
  1: list<FilenodeInfo> Data,
  2: list<i64> Pointers,
}

struct FilenodeInfo {
  1: required mononoke_types_thrift.RepoPath path,
  2: required mercurial_thrift.HgNodeHash filenode,
  3: optional mercurial_thrift.HgNodeHash p1,
  4: optional mercurial_thrift.HgNodeHash p2,
  5: optional FilenodeCopyFrom copyfrom,
  6: required mercurial_thrift.HgNodeHash linknode,
}

struct FilenodeCopyFrom {
  1: required mononoke_types_thrift.RepoPath path,
  2: required mercurial_thrift.HgNodeHash filenode,
}
