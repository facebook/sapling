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
  $ quiet testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > EOF

start mononoke

  $ start_and_wait_for_mononoke_server
  $ wait_for_mononoke_cache_warmup
