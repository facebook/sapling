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
  $ gitimport "$GIT_REPO" --derive-trees --derive-hg --hggit-compatibility --bonsai-git-mapping full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * GitRepo:*repo-git commit 1 of 1 - Oid:* => Bid:* (glob)
  * 1 tree(s) are valid! (glob)
  * Hg: 8ce3eae44760b500bf3f2c3922a95dcd3c908e9e: HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(d4229e9850e9244c3a986a62590ffada646e7200593bc26e4cc8c9aa10730a26))) (glob)

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
  $ gitimport "$GIT_REPO" --bonsai-git-mapping missing-for-commit e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * GitRepo:*repo-git commit 1 of 1 - Oid:* => Bid:* (glob)
  * Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(4b33fb0ff41a199456fc270c2eceb5f73eec97432c1fd4a4e56b15c48c4fc6dd))) (glob)

# Test missing-for-commit flag (agains fully imported repo history)
  $ gitimport "$GIT_REPO" --suppress-ref-mapping --bonsai-git-mapping missing-for-commit e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * Nothing to import for repo *repo-git. (glob)

# Also check that a readonly import works
  $ gitimport "$GIT_REPO" --with-readonly-storage=true --derive-trees --derive-hg --hggit-compatibility full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * GitRepo:*repo-git commit 1 of 2 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 2 - Oid:* => Bid:* (glob)
  * 2 tree(s) are valid! (glob)
  * Hg: 8ce3eae44760b500bf3f2c3922a95dcd3c908e9e: HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Hg: e8615d6f149b876be0a2f30a1c5bf0c42bf8e136: HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(*))) (glob)

# Set master (gitimport does not do this yet)
  $ mononoke_admin bookmarks set master 4b33fb0ff41a199456fc270c2eceb5f73eec97432c1fd4a4e56b15c48c4fc6dd
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(*)) (glob)
  * Current position of BookmarkName { bookmark: "master" } is None (glob)

# Start Mononoke
  $ mononoke
  $ wait_for_mononoke

# Clone the repository
  $ cd "$TESTTMP"
  $ hgmn_clone 'ssh://user@dummy/repo' "$HG_REPO"
  $ cd "$HG_REPO"
  $ cat "file1"
  this is file1
  $ cat "file2"
  this is file2

