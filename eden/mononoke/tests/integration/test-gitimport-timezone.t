# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"

# Setup git repsitory
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -am "Add file1" --date="1000000000 -0800"
  [master (root-commit) 200c0e8] Add file1
   Date: Sat Sep 8 17:46:40 2001 -0800
   1 file changed, 1 insertion(+)
   create mode 100644 file1
  $ git log
  commit 200c0e8395a7222c38cf9c3efdf734d2507fda90
  Author: mononoke <mononoke@mononoke>
  Date:   Sat Sep 8 17:46:40 2001 -0800
  
      Add file1

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --derive-hg full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 1 - Oid:* => Bid:* (glob)
  * Hg: Sha1(200c0e8395a7222c38cf9c3efdf734d2507fda90): HgManifestId(HgNodeHash(Sha1(009adbc8d457927d2e1883c08b0692bc45089839))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(d37ab14503b5323dd32b54f6b1da45c3e8add4dce31d6d28da89b9f3f27550b3))) (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master d37ab14503b5323dd32b54f6b1da45c3e8add4dce31d6d28da89b9f3f27550b3
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Current position of BookmarkName { bookmark: "master" } is None (glob)

# Start Mononoke
  $ start_and_wait_for_mononoke_server
# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone mononoke://$(mononoke_address)/repo "$HG_REPO"
  $ cd "$HG_REPO"
  $ cat "file1"
  this is file1
  $ hg log -r master
  commit:      89e61a7d29d5
  bookmark:    master
  bookmark:    default/master
  hoistedname: master
  user:        mononoke <mononoke@mononoke>
  date:        Sat Sep 08 17:46:40 2001 -0800
  summary:     Add file1
  
