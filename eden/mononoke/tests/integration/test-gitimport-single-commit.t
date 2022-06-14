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
  $ git commit -am "Add file1"
  [master (root-commit) 8ce3eae] Add file1
   1 file changed, 1 insertion(+)
   create mode 100644 file1
  $ mkdir dir
  $ echo "dir/file2" > dir/file2
  $ echo "file3" > file3
  $ echo "filetoremove" > filetoremove
  $ git add dir/file2 file3 filetoremove
  $ git commit -aqm "Add 3 more files"
  $ git rm filetoremove
  rm 'filetoremove'
  $ git commit -aqm "Remove one file"
  $ git log HEAD -n 1 --pretty=oneline
  69d481cfc9a21ef59b516c3de04cd742d059d345 Remove one file

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" import-tree-as-single-bonsai-changeset 69d481cfc9a21ef59b516c3de04cd742d059d345
  * using repo "repo" repoid RepositoryId(0) (glob)
  * imported as 996a9fdfbf6ef7fe0e61e6f5da99f2189896379558cc24e9501b06b45350d489 (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 996a9fdfbf6ef7fe0e61e6f5da99f2189896379558cc24e9501b06b45350d489
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Current position of BookmarkName { bookmark: "master" } is None (glob)

# Start Mononoke
  $ start_and_wait_for_mononoke_server
# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone mononoke://$(mononoke_address)/repo "$HG_REPO"
  $ cd "$HG_REPO"
  $ hgmn up -q master
  $ cat file1
  this is file1
  $ cat dir/file2
  dir/file2
  $ cat file3
  file3
  $ [[ -e filetoremove ]]
  [1]
