# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export CACHE_WARMUP_BOOKMARK="master_bookmark"
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo

setup hg server repo
  $ cd repo
  $ echo a > a && hg add a && hg ci -m a

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo/.hg repo

start mononoke

  $ start_and_wait_for_mononoke_server
  $ wait_for_mononoke_cache_warmup
