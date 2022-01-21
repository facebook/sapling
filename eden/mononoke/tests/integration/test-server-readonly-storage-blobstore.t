# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark
  $ hg bookmark master_bookmark -r tip

blobimport normally, so sql is populated
  $ cd ..
  $ blobimport repo-hg/.hg repo

blobimport, check blobstore puts are blocked
  $ rm -rf "$TESTTMP/blobstore/blobs/"*content*
  $ blobimport repo-hg/.hg repo --with-readonly-storage=true | grep 'root cause:'
  * root cause: ReadOnlyPut("*") (glob)
