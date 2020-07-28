# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"

# Setup git repository
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init
  Initialized empty Git repository in $TESTTMP/repo-git/.git/
  $ echo "this is file1" > file1
  $ mkdir file2_repo
  $ cd file2_repo
  $ echo "this is file2" > file2
  $ cd ..
  $ git add file1 file2_repo/file2
  $ git commit -am "Add file1 and file2"
  [master (root-commit) ce435b0] Add file1 and file2
   2 files changed, 2 insertions(+)
   create mode 100644 file1
   create mode 100644 file2_repo/file2
  $ mkdir file3_repo
  $ echo "this is file3" > file3_repo/file3
  $ git add file3_repo/file3
  $ git commit -am "Add file3"
  [master 2c01e4a] Add file3
   1 file changed, 1 insertion(+)
   create mode 100644 file3_repo/file3

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ repo_import "$GIT_REPO" --dest-path "new_dir/new_repo" --batch-size 3 --bookmark-suffix "new_repo" --disable-phabricator-check
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Created ce435b03d4ef526648f8654c61e26ae5cc1069cc => ChangesetId(Blake2(f7cbf75d9c08ff96896ed2cebd0327aa514e58b1dd9901d50129b9e08f4aa062)) (glob)
  * Created 2c01e4a5658421e2bfcd08e31d9b69399319bcd3 => ChangesetId(Blake2(f7708ed066b1c23591f862148e0386ec704a450e572154cc52f87ca0e394a0fb)) (glob)
  * 2 bonsai changesets have been committed (glob)
  * Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(f7708ed066b1c23591f862148e0386ec704a450e572154cc52f87ca0e394a0fb))) (glob)
  * Remapped ChangesetId(Blake2(f7cbf75d9c08ff96896ed2cebd0327aa514e58b1dd9901d50129b9e08f4aa062)) => ChangesetId(Blake2(a159bc614d2dbd07a5ecc6476156fa464b69e884d819bbc2e854ade3e4c353b9)) (glob)
  * Remapped ChangesetId(Blake2(f7708ed066b1c23591f862148e0386ec704a450e572154cc52f87ca0e394a0fb)) => ChangesetId(Blake2(a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5)) (glob)
  * Created bookmark BookmarkName { bookmark: "repo_import_new_repo" } pointing to * (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to * (glob)

# Check if we derived all the types
  $ BOOKMARK_NAME="repo_import_new_repo"
  $ mononoke_admin derived-data exists changeset_info $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists blame $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists deleted_manifest $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists fastlog $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists filenodes $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists fsnodes $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists hgchangesets $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists unodes $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5

# Start Mononoke
  $ mononoke
  $ wait_for_mononoke

# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone 'ssh://user@dummy/repo' "$HG_REPO"
  $ cd "$HG_REPO"
  $ cat "new_dir/new_repo/file1"
  this is file1
  $ cat "new_dir/new_repo/file2_repo/file2"
  this is file2
  $ cat "new_dir/new_repo/file3_repo/file3"
  this is file3
