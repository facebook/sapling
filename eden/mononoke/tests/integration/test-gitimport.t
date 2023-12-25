# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLED_DERIVED_DATA='["git_trees", "filenodes", "hgchangesets"]' setup_common_config
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ cat >> repos/repo/server.toml <<EOF
  > [source_control_service]
  > permit_writes = true
  > EOF

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -am "Add file1"
  [master (root-commit) 8ce3eae] Add file1
   1 file changed, 1 insertion(+)
   create mode 100644 file1
  $ git tag -a -m"new tag" first_tag
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.


# Import it into Mononoke
  $ cd "$TESTTMP"
  $ gitimport --record-head-symref "$GIT_REPO" --concurrency 100 --derive-hg full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 1 - Oid:* => Bid:* (glob)
  * Hg: Sha1(8ce3eae44760b500bf3f2c3922a95dcd3c908e9e): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/remotes/origin/HEAD": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/remotes/origin/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)

# Validate if creating the commit also uploaded the raw commit blob AND the raw tree blob
# The ids of the blobs should be the same as the commit and tree object ids
  $ ls $TESTTMP/blobstore/blobs | grep "git_object"
  blob-repo0000.git_object.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_object.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb

# Validate if creating the commit also uploaded the packfile items for the imported git objects
  $ ls $TESTTMP/blobstore/blobs | grep "git_packfile_base_item"
  blob-repo0000.git_packfile_base_item.433eb172726bc7b6d60e8d68efb0f0ef4e67a667
  blob-repo0000.git_packfile_base_item.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_packfile_base_item.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb

# Validate if we imported the HEAD symref
  $ mononoke_newadmin git-symref -R repo get --symref-name HEAD
  The symbolic ref HEAD points to branch master

# Cross reference with the blobs present in the git store
  $ cd "$GIT_REPO"
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep -e '^[^ ]* commit' -e '^[^ ]* tree' | cut -d" " -f1,9- | sort
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb

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
  * GitRepo:*repo-git commit 1 of 2 - Oid:* => Bid:* (already exists) (glob)
  * GitRepo:*repo-git commit 2 of 2 - Oid:* => Bid:* (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c))) (glob)
  * Ref: "refs/remotes/origin/HEAD": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/remotes/origin/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)

# Validate if creating the commit also uploaded the raw commit blob
# The id of the blob should be the same as the commit object id
  $ ls $TESTTMP/blobstore/blobs | grep "git_object"
  blob-repo0000.git_object.7327e6c9b533787eeb80877d557d50f39c480f54
  blob-repo0000.git_object.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_object.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb
  blob-repo0000.git_object.e8615d6f149b876be0a2f30a1c5bf0c42bf8e136

# Cross reference with the blobs present in the git store
  $ cd "$GIT_REPO"
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep -e '^[^ ]* commit' -e '^[^ ]* tree' | cut -d" " -f1,9- | sort
  7327e6c9b533787eeb80877d557d50f39c480f54
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb
  e8615d6f149b876be0a2f30a1c5bf0c42bf8e136

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
  * Ref: "refs/remotes/origin/HEAD": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/remotes/origin/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)

# Add an empty tag and a simple tag (i.e. non-annotated tag)
  $ cd "$GIT_REPO"
  $ git tag -a empty_tag -m ""
  $ git tag simple_tag
# Check its ref can be parsed
  $ cd "$TESTTMP"
  $ gitimport "$GIT_REPO" --generate-bookmarks full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 2 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 2 - Oid:* => Bid:* (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/HEAD": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/remotes/origin/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/tags/empty_tag": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c))) (glob)
  * Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/tags/simple_tag": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c))) (glob)
  * Initializing repo: repo (glob)
  * Initialized repo: repo (glob)
  * All repos initialized. * (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (created) (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (moved from ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c))) (glob)
  * Bookmark: "tags/empty_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (created) (glob)
  * Bookmark: "tags/first_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created) (glob)
  * Bookmark: "tags/simple_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (created) (glob)

# Generating bookmarks should upload the raw tag object to blobstore.
# The id of the blob should be the same as the tag object id
  $ ls $TESTTMP/blobstore/blobs | grep "git_object"
  blob-repo0000.git_object.7327e6c9b533787eeb80877d557d50f39c480f54
  blob-repo0000.git_object.8963e1f55d1346a07c3aec8c8fc72bf87d0452b1
  blob-repo0000.git_object.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_object.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb
  blob-repo0000.git_object.e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  blob-repo0000.git_object.fb02ed046a1e75fe2abb8763f7c715496ae36353

# Generating bookmarks should upload the packfile base item for the git tag object to blobstore.
  $ ls $TESTTMP/blobstore/blobs | grep "git_packfile_base_item"
  blob-repo0000.git_packfile_base_item.433eb172726bc7b6d60e8d68efb0f0ef4e67a667
  blob-repo0000.git_packfile_base_item.7327e6c9b533787eeb80877d557d50f39c480f54
  blob-repo0000.git_packfile_base_item.8963e1f55d1346a07c3aec8c8fc72bf87d0452b1
  blob-repo0000.git_packfile_base_item.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_packfile_base_item.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb
  blob-repo0000.git_packfile_base_item.e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  blob-repo0000.git_packfile_base_item.f138820097c8ef62a012205db0b1701df516f6d5
  blob-repo0000.git_packfile_base_item.fb02ed046a1e75fe2abb8763f7c715496ae36353

# Cross reference with the tag blobs present in the git store
# (There should be two, first_tag and empty_tag)
  $ cd "$GIT_REPO"
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep -e '^[^ ]* tag' | cut -d" " -f1,9- | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1
  fb02ed046a1e75fe2abb8763f7c715496ae36353

# Generating bookmarks should also capture the tag mapping in bonsai_tag_mapping table
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash FROM bonsai_tag_mapping"
  tags/empty_tag|D5BE6FDF77FC73EE5E3A4BAB1ADBB4772829E06C0F104E6CC0D70CABF1EBFF4B|FB02ED046A1E75FE2ABB8763F7C715496AE36353
  tags/first_tag|5CA579C0E3EBEA708371B65CE559E5A51B231AD1B6F3CDFD874CA27362A2A6A8|8963E1F55D1346A07C3AEC8C8FC72BF87D0452B1

# Generating bookmarks should also create the changeset corresponding to the
# git tag at Mononoke end
  $ ls $TESTTMP/blobstore/blobs | grep -e d5be6fdf77fc73ee5e3a4bab1adbb4772829e06c0f104e6cc0d70cabf1ebff4b -e 5ca579c0e3ebea708371b65ce559e5a51b231ad1b6f3cdfd874ca27362a2a6a8
  blob-repo0000.changeset.blake2.5ca579c0e3ebea708371b65ce559e5a51b231ad1b6f3cdfd874ca27362a2a6a8
  blob-repo0000.changeset.blake2.d5be6fdf77fc73ee5e3a4bab1adbb4772829e06c0f104e6cc0d70cabf1ebff4b

# Validate if the mapping from git tag to its metadata changeset was created
# at Mononoke end
  $ mononoke_newadmin bookmarks -R repo get tags/first_tag --category tag
  Metadata changeset for tag bookmark tags/first_tag: 
  5ca579c0e3ebea708371b65ce559e5a51b231ad1b6f3cdfd874ca27362a2a6a8
  Changeset pointed to by the tag bookmark tags/first_tag
  032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044

  $ mononoke_newadmin bookmarks -R repo get tags/empty_tag --category tag
  Metadata changeset for tag bookmark tags/empty_tag: 
  d5be6fdf77fc73ee5e3a4bab1adbb4772829e06c0f104e6cc0d70cabf1ebff4b
  Changeset pointed to by the tag bookmark tags/empty_tag
  da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c

# This should show up as empty since we don't record simple tags as tags in the bookmarks table
  $ mononoke_newadmin bookmarks -R repo get tags/simple_tag --category tag
  (not set)

# Importing a second time should still work
  $ gitimport "$GIT_REPO" --generate-bookmarks full-repo
  * using repo "repo" repoid RepositoryId(0) (glob)
  * GitRepo:*repo-git commit 1 of 2 - Oid:* => Bid:* (glob)
  * GitRepo:*repo-git commit 2 of 2 - Oid:* => Bid:* (glob)
  * Ref: "refs/heads/master": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/remotes/origin/HEAD": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/remotes/origin/master": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Ref: "refs/tags/empty_tag": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(*))) (glob)
  * Ref: "refs/tags/simple_tag": Some(ChangesetId(Blake2(*))) (glob)
  * Initializing repo: repo (glob)
  * Initialized repo: repo (glob)
  * All repos initialized. * (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (moved from ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044))) (glob)
  * Bookmark: "heads/master": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (moved from ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c))) (glob)
  * Bookmark: "tags/empty_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (already up-to-date) (glob)
  * Bookmark: "tags/first_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date) (glob)
  * Bookmark: "tags/simple_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (already up-to-date) (glob)


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
  $ hg log --config extensions.gitrevset= --template 'hg={node}: git={gitnode}\nextras=(\n{extras % "  {extra}\n"})\n' -r heads/master
  hg=b48ed460078564067eabec6c4a50909d000d7e22: git=8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  extras=(
    branch=default
    convert_revision=8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
    hg-git-rename-source=git
  )

# Checks all the bookmarks were created
  $ hg bookmarks --all
  * heads/master * (glob)
  * tags/empty_tag * e7f52161c612 (glob)
  * tags/first_tag * b48ed4600785 (glob)
  * tags/simple_tag * e7f52161c612 (glob)
