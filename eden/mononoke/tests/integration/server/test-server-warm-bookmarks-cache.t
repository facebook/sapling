# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

Setup repository

  $ SCUBA_LOGGING_PATH="$TESTTMP/scuba.json"
  $ export REPO_CLIENT_USE_WARM_BOOKMARKS_CACHE="true"
  $ BLOB_TYPE="blob_files" quiet default_setup_pre_blobimport

  $ blobimport repo/.hg repo
  $ mononoke --scuba-dataset "file://$SCUBA_LOGGING_PATH"
  $ wait_for_mononoke "$TESTTMP/repo"
  $ cd "$TESTTMP"
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2 || exit 1
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ cd "$TESTTMP/repo2"
  $ hg pull -q
  $ hg log -r "master_bookmark" -T '{desc}\n'
  C

  $ hg up -q 0
  $ echo a >> anotherfile
  $ hg add anotherfile
  $ hg ci -m 'new commit'
  $ hg log -r master_bookmark -T '{node}\n'
  26805aba1e600a82e93661149f2313866a221a7b
  $ hg push -r . --to master_bookmark
  pushing rev b1673e56df82 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hg log -r master_bookmark -T '{node}\n'
  3dee7c6d777101a0f12a87a1394b35b4a249c700

  $ sleep 2
  $ grep "Fetching bookmarks from Warm bookmarks cache" "$SCUBA_LOGGING_PATH" | wc -l
  3

  $ hg pull -q
  $ grep "Fetching bookmarks from Warm bookmarks cache" "$SCUBA_LOGGING_PATH" | wc -l
  4
