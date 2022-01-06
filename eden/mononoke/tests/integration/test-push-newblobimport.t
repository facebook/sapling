# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client --noupdate

  $ cd repo-hg
  $ touch a
  $ hg add a
  $ hg ci -ma
  $ echo b > b
  $ hg add b
  $ hg ci -mb

setup master bookmark

  $ hg bookmark master_bookmark -r 'tip'

verify content

  $ hg log -r ::. -T '{node}: {files}\n'
  3903775176ed42b1458a6281db4a0ccf4d9f287a: a
  c201a1696ba0db28be95eedf0949329fa8c44478: b
  $ hg log
  commit:      c201a1696ba0
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
   (re)
  commit:      3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ mononoke
  $ wait_for_mononoke

  $ cd client
  $ echo 'remotefilelog' >> .hg/requires
  $ hgmn pull --config ui.disable-stream-clone=true -q
  warning: stream clone is disabled
  $ hgmn up c201a1696ba0db28be95eedf0949329fa8c44478 -q
  $ cat a
  $ cat b
  b
  $ hg log -r ::. -T '{node}: {files}\n'
  3903775176ed42b1458a6281db4a0ccf4d9f287a: a
  c201a1696ba0db28be95eedf0949329fa8c44478: b
