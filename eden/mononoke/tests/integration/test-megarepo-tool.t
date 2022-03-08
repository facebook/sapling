# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOTYPE="blob_files"
  $ REPOID=0 REPONAME=repo setup_common_config $REPOTYPE
  $ REPOID=1 REPONAME=repo1 setup_common_config $REPOTYPE
  $ REPOID=2 REPONAME=repo2 setup_common_config $REPOTYPE
  $ setup_commitsyncmap

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ function createfile { mkdir -p "$(dirname  $1)" && echo "$1" > "$1" && hg add "$1"; }

-- create some semblance of fbsource
  $ createfile fbcode/fbcodfile_fbsource
  $ createfile fbobjc/fbobjcfile_fbsource
  $ createfile fbandroid/fbandroidfile_fbsource
  $ createfile xplat/xplatfile_fbsource
  $ createfile arvr/arvrfile_fbsource
  $ createfile third-party/thirdpartyfile_fbsource
  $ hg ci -m "fbsource-like commit"
  $ hg book -r . fbsource_master

-- create some semblance of ovrsource
  $ hg up null -q
  $ createfile fbcode/fbcodfile_ovrsource
  $ createfile fbobjc/fbobjcfile_ovrsource
  $ createfile fbandroid/fbandroidfile_ovrsource
  $ createfile xplat/xplatfile_ovrsource
  $ createfile arvr/arvrfile_ovrsource
  $ createfile third-party/thirdpartyfile_ovrsource
  $ createfile Software/softwarefile_ovrsource
  $ createfile Research/researchfile_ovrsource
  $ hg ci -m "ovrsource-like commit"
  $ hg book -r . ovrsource_master

  $ hg log -T "{node} {bookmarks}\n" -r "all()"
  4da689e6447cf99bbc121eaa7b05ea1504cf2f7c fbsource_master
  4d79e7d65a781c6c80b3ee4faf63452e8beafa97 ovrsource_master

  $ cd $TESTTMP

setup repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport
  $ blobimport repo-hg/.hg repo

  $ export COMMIT_DATE="1985-09-04T00:00:00.00Z"
move things in fbsource
  $ RUST_BACKTRACE=1 megarepo_tool move 1 fbsource_master user "fbsource move" --mark-public --commit-date-rfc3339 "$COMMIT_DATE" --bookmark fbsource_move --mapping-version-name TEST_VERSION_NAME
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "fbsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "fbsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of *: HgChangesetId(HgNodeHash(Sha1(*))) (glob)

move things in ovrsource in a stack
  $ megarepo_tool move 2 ovrsource_master user "ovrsource stack move" --mark-public --commit-date-rfc3339 "$COMMIT_DATE" --max-num-of-moves-in-commit 1 --bookmark ovrsource_move --mapping-version-name TEST_VERSION_NAME
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "ovrsource_move" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of * is: * (glob)
  * created 7 commits, with the last commit * (glob)

merge things in both repos
  $ megarepo_tool merge fbsource_move ovrsource_move user "megarepo merge" --mark-public --commit-date-rfc3339 "$COMMIT_DATE" --bookmark master
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: * (glob)
  * changeset resolved as: * (glob)
  * Creating a merge commit (glob)
  * Checking if there are any path conflicts (glob)
  * Done checking path conflicts (glob)
  * Creating a merge bonsai changeset with parents: * (glob)
  * Marked as public * (glob)
  * Setting bookmark BookmarkName { bookmark: "master" } to point to * (glob)
  * Setting bookmark BookmarkName { bookmark: "master" } finished (glob)
  * Generating an HG equivalent of * (glob)
  * Hg equivalent of *: HgChangesetId(HgNodeHash(Sha1(*))) (glob)

start mononoke server
  $ start_and_wait_for_mononoke_server
pull the result
  $ cd $TESTTMP/repo-pull
  $ hgmn -q pull && hgmn -q up master
  $ ls -1
  arvr
  arvr-legacy
  fbandroid
  fbcode
  fbobjc
  third-party
  xplat
  $ ls -1 fbcode fbandroid fbobjc xplat arvr arvr-legacy
  arvr:
  arvrfile_ovrsource
  
  arvr-legacy:
  Research
  Software
  third-party
  
  fbandroid:
  fbandroidfile_fbsource
  
  fbcode:
  fbcodfile_fbsource
  
  fbobjc:
  fbobjcfile_fbsource
  
  xplat:
  xplatfile_fbsource

test pre-merge deletes functionality
  $ cd "$TESTTMP"
  $ megarepo_tool pre-merge-delete master author "merge preparation" --even-chunk-size 4 --commit-date-rfc3339 "$COMMIT_DATE" 2>/dev/null
  342cc9bff69c2f71d19698aa1f48d5d260804f4b063625143dd7052768c0df24
  1e80c201efb51b8e99fc4b16baa1ef389cf53faec8be2d42f6efb47396e20756
  a201ac038c63e32c17457f830731bb4fed74650856a75351b15c0a5f7549554d
