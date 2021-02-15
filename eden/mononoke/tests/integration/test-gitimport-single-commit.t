# Copyright (c) Facebook, Inc. and its affiliates.
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
  $ gitimport "$GIT_REPO"  --bonsai-git-mapping import-tree-as-single-bonsai-changeset 69d481cfc9a21ef59b516c3de04cd742d059d345
  * using repo "repo" repoid RepositoryId(0) (glob)
  * found 3 file paths (glob)
  * imported as 4e4aec4571f5f1cafabf6e968c11ee99f3a5deab1292b23f6faddbbf1e19422e (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 4e4aec4571f5f1cafabf6e968c11ee99f3a5deab1292b23f6faddbbf1e19422e
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Current position of BookmarkName { bookmark: "master" } is None (glob)

# Start Mononoke
  $ mononoke
  $ wait_for_mononoke

# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone 'ssh://user@dummy/repo' "$HG_REPO"
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
