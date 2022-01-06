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
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  commit:      0e7ec5675652
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

setup push source repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2

start mononoke

  $ mononoke
  $ wait_for_mononoke

create new commit in repo2 and check that push fails

  $ cd repo2
  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb --extra "change-xrepo-mapping-to-version=somemapping"

  $ hgmn push ssh://user@dummy/repo -r . --to master_bookmark --config extensions.remotenames=
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Disallowed extra change-xrepo-mapping-to-version is set on * (glob)
  remote: 
  remote:   Caused by:
  remote:     Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)
  remote:   Caused by:
  remote:     Disallowed extra change-xrepo-mapping-to-version is set on * (glob)
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to fast-forward bookmark (set pushvar NON_FAST_FORWARD=true for a non-fast-forward move)",
  remote:             source: Error(
  remote:                 "Disallowed extra change-xrepo-mapping-to-version is set on *", (glob)
  remote:             ),
  remote:         },
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ killandwait $MONONOKE_PID
  $ cd "$TESTTMP"
  $ rm -rf "$TESTTMP/mononoke-config"
  $ ALLOW_CHANGE_XREPO_MAPPING_EXTRA=true setup_common_config
  $ mononoke
  $ wait_for_mononoke
  $ cd "$TESTTMP/repo2"
  $ hgmn push ssh://user@dummy/repo -r . --to master_bookmark --config extensions.remotenames=
  pushing rev 9c40727be57c to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
