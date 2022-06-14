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

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --derive-hg full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 1 - Oid:* => Bid:* (glob)
  * Hg: Sha1(8ce3eae44760b500bf3f2c3922a95dcd3c908e9e): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)

# Add second commit to git repository
  $ cd "$GIT_REPO"
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -am "Add file2"
  [master e8615d6] Add file2
   1 file changed, 1 insertion(+)
   create mode 100644 file2

# Test missing-for-commit flag (against partially imported repo history)
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" missing-for-commit e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 1 - Oid:* => Bid:* (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c))) (glob)

# Test missing-for-commit flag (agains fully imported repo history)
  $ gitimport "$GIT_REPO" --suppress-ref-mapping missing-for-commit e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Nothing to import for repo *repo-git. (glob)

# Also check that a readonly import works
  $ gitimport "$GIT_REPO" --with-readonly-storage=true --derive-hg full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 2 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 2 - Oid:* => Bid:* (glob)
  * Hg: Sha1(8ce3eae44760b500bf3f2c3922a95dcd3c908e9e): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Hg: Sha1(e8615d6f149b876be0a2f30a1c5bf0c42bf8e136): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(*))) (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c
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
  $ cat "file2"
  this is file2

# Check that we can see the git hash from extras
  $ hg log --config extensions.gitrevset= --template 'hg={node}: git={gitnode}\nextras=(\n{extras % "  {extra}\n"})\n' -r master
  hg=e7f52161c6127445391295b677f87aded035450a: git=e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  extras=(
    branch=default
    convert_revision=e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
    hg-git-rename-source=git
  )
