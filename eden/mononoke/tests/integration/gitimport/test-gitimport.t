# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo"
  $ setup_common_config blob_files

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m"new tag" first_tag
  $ git tag -a -m "changing tag" changing_tag
  $ tagged_commit=$(git rev-parse HEAD)
# Create a recursive tag to check if it gets imported
  $ git config advice.nestedTag false
  $ git tag -a recursive_tag -m "this recursive tag points to first_tag" $(git rev-parse first_tag)
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.
  $ cd repo-git
  $ git fetch "$GIT_REPO_ORIGIN" +refs/*:refs/* --prune -u
  From $TESTTMP/origin/repo-git
   - [deleted]         (none)     -> origin/master_bookmark
     (refs/remotes/origin/HEAD has become dangling)
  $ git branch "a_ref_prefixed_by_remotes_origin"
  $ git update-ref refs/remotes/origin/a_ref_prefixed_by_remotes_origin a_ref_prefixed_by_remotes_origin
  $ git branch -d a_ref_prefixed_by_remotes_origin
  Deleted branch a_ref_prefixed_by_remotes_origin (was 8ce3eae).
  $ cd ..


# Import it into Mononoke
  $ cd "$TESTTMP"
  $ with_stripped_logs gitimport "$GIT_REPO" --concurrency 100 --derive-hg full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git commit 1 of 1 - Oid:8ce3eae4 => Bid:032cd4dc* (glob)
  Hg: Sha1(8ce3eae44760b500bf3f2c3922a95dcd3c908e9e): HgManifestId(HgNodeHash(Sha1(009adbc8d457927d2e1883c08b0692bc45089839)))
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/remotes/origin/a_ref_prefixed_by_remotes_origin": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/changing_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/recursive_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))

# Validate if creating the commit also uploaded the raw commit blob AND the raw tree blob
# The ids of the blobs should be the same as the commit and tree object ids
  $ ls $TESTTMP/blobstore/blobs | grep "git_object"
  blob-repo0000.git_object.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_object.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb

# Validate that we are able to view the git objects stored in mononoke store
  $ mononoke_admin git-objects -R repo fetch --id 8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  The object is a Git Commit
  
  Commit {
      tree: Sha1(cb2ef838eb24e4667fee3a8b89c930234ae6e4bb),
      parents: [],
      author: Signature {
          name: "mononoke",
          email: "mononoke@mononoke",
          time: Time {
              seconds: 946684800,
              offset: 0,
          },
      },
      committer: Signature {
          name: "mononoke",
          email: "mononoke@mononoke",
          time: Time {
              seconds: 946684800,
              offset: 0,
          },
      },
      encoding: None,
      message: "Add file1\n",
      extra_headers: [],
  }

# Validate if creating the commit also uploaded the packfile items for the imported git objects
  $ ls $TESTTMP/blobstore/blobs | grep "git_packfile_base_item"
  blob-repo0000.git_packfile_base_item.433eb172726bc7b6d60e8d68efb0f0ef4e67a667
  blob-repo0000.git_packfile_base_item.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_packfile_base_item.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb

# Validate if we imported the HEAD symref
  $ mononoke_admin git-symref -R repo get --symref-name HEAD
  The symbolic ref HEAD points to branch master_bookmark

# Cross reference with the blobs present in the git store
  $ cd "$GIT_REPO"
  $ git rev-list --objects --all | git cat-file --batch-check='%(objectname) %(objecttype) %(rest)' | grep -e '^[^ ]* commit' -e '^[^ ]* tree' | cut -d" " -f1,9- | sort
  8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  cb2ef838eb24e4667fee3a8b89c930234ae6e4bb

# Add second commit to git repository
  $ cd "$GIT_REPO"
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"

# Test missing-for-commit flag (against partially imported repo history)
  $ cd "$TESTTMP"
  $ with_stripped_logs gitimport "$GIT_REPO" missing-for-commit e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git 1 of 2 commit(s) already exist* (glob)
  GitRepo:$TESTTMP/repo-git commit 2 of 2 - Oid:e8615d6f => Bid:da93dc81* (glob)
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Ref: "refs/remotes/origin/a_ref_prefixed_by_remotes_origin": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/changing_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/recursive_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))

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
  $ with_stripped_logs gitimport "$GIT_REPO" --suppress-ref-mapping missing-for-commit e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  using repo "repo" repoid RepositoryId(0)
  Nothing to import for repo $TESTTMP/repo-git.

# Also check that a readonly import works
  $ with_stripped_logs gitimport "$GIT_REPO" --with-readonly-storage=true --derive-hg --skip-head-symref full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git 2 of 2 commit(s) already exist* (glob)
  Hg: Sha1(*): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  Hg: Sha1(*): HgManifestId(HgNodeHash(Sha1(*))) (glob)
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Ref: "refs/remotes/origin/a_ref_prefixed_by_remotes_origin": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/changing_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/recursive_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))

# Add an empty tag and a simple tag (i.e. non-annotated tag)
  $ cd "$GIT_REPO"
  $ git tag -a empty_tag -m ""
  $ git tag simple_tag

# Check its ref can be parsed
  $ cd "$TESTTMP"
  $ with_stripped_logs gitimport "$GIT_REPO" --generate-bookmarks full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git 2 of 2 commit(s) already exist* (glob)
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Ref: "refs/remotes/origin/a_ref_prefixed_by_remotes_origin": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/changing_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/empty_tag": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/recursive_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/simple_tag": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Initializing repo: repo
  Initialized repo: repo
  All repos initialized. It took: * seconds (glob)
  Bookmark: "heads/master_bookmark": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (created)
  Bookmark: "remotes/origin/a_ref_prefixed_by_remotes_origin": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "tags/changing_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "tags/empty_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (created)
  Bookmark: "tags/first_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "tags/recursive_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (created)
  Bookmark: "tags/simple_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (created)

# Generating bookmarks should upload the raw tag object to blobstore.
# The id of the blob should be the same as the tag object id
  $ ls $TESTTMP/blobstore/blobs | grep "git_object"
  blob-repo0000.git_object.18b57eb6e2869701c04ee36399fcde1a824a00dd
  blob-repo0000.git_object.5733f87f2aa7e68c86273960497f2d81e11c6c8e
  blob-repo0000.git_object.7327e6c9b533787eeb80877d557d50f39c480f54
  blob-repo0000.git_object.8963e1f55d1346a07c3aec8c8fc72bf87d0452b1
  blob-repo0000.git_object.8ce3eae44760b500bf3f2c3922a95dcd3c908e9e
  blob-repo0000.git_object.cb2ef838eb24e4667fee3a8b89c930234ae6e4bb
  blob-repo0000.git_object.e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  blob-repo0000.git_object.fb02ed046a1e75fe2abb8763f7c715496ae36353

# Generating bookmarks should upload the packfile base item for the git tag object to blobstore.
  $ ls $TESTTMP/blobstore/blobs | grep "git_packfile_base_item"
  blob-repo0000.git_packfile_base_item.18b57eb6e2869701c04ee36399fcde1a824a00dd
  blob-repo0000.git_packfile_base_item.433eb172726bc7b6d60e8d68efb0f0ef4e67a667
  blob-repo0000.git_packfile_base_item.5733f87f2aa7e68c86273960497f2d81e11c6c8e
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
  18b57eb6e2869701c04ee36399fcde1a824a00dd
  5733f87f2aa7e68c86273960497f2d81e11c6c8e
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1
  fb02ed046a1e75fe2abb8763f7c715496ae36353

# Generating bookmarks should also capture the tag mapping in bonsai_tag_mapping table
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash, target_is_tag FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/changing_tag|68710F4158EB1188BBAC3955AEA66A944818578BCE93514E68CD627752243BE9|5733F87F2AA7E68C86273960497F2D81E11C6C8E|0
  tags/empty_tag|1910A71753B6A3F0A308C44E85AE28EB57272D5519D53C4577AF4395784EFDB3|FB02ED046A1E75FE2ABB8763F7C715496AE36353|0
  tags/first_tag|5CA579C0E3EBEA708371B65CE559E5A51B231AD1B6F3CDFD874CA27362A2A6A8|8963E1F55D1346A07C3AEC8C8FC72BF87D0452B1|0
  tags/recursive_tag|1AB4E7C855BE1F10B2A3E48A398B7B068EFB96EE81A75B8F74654B521D28A988|18B57EB6E2869701C04EE36399FCDE1A824A00DD|1

# Generating bookmarks should also create the changeset corresponding to the
# git tag at Mononoke end
  $ ls $TESTTMP/blobstore/blobs | grep -e changeset.blake2
  blob-repo0000.changeset.blake2.032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044
  blob-repo0000.changeset.blake2.1910a71753b6a3f0a308c44e85ae28eb57272d5519d53c4577af4395784efdb3
  blob-repo0000.changeset.blake2.1ab4e7c855be1f10b2a3e48a398b7b068efb96ee81a75b8f74654b521d28a988
  blob-repo0000.changeset.blake2.5ca579c0e3ebea708371b65ce559e5a51b231ad1b6f3cdfd874ca27362a2a6a8
  blob-repo0000.changeset.blake2.68710f4158eb1188bbac3955aea66a944818578bce93514e68cd627752243be9
  blob-repo0000.changeset.blake2.da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c

# Validate if the mapping from git tag to its metadata changeset was created
# at Mononoke end
  $ mononoke_admin bookmarks -R repo get tags/first_tag
  Metadata changeset for tag bookmark tags/first_tag: 
  5ca579c0e3ebea708371b65ce559e5a51b231ad1b6f3cdfd874ca27362a2a6a8
  Changeset pointed to by the tag bookmark tags/first_tag
  032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044

  $ mononoke_admin bookmarks -R repo get tags/empty_tag
  Metadata changeset for tag bookmark tags/empty_tag: 
  1910a71753b6a3f0a308c44e85ae28eb57272d5519d53c4577af4395784efdb3
  Changeset pointed to by the tag bookmark tags/empty_tag
  da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c

# This should return an emtpy metadata changeset since we don't store those for simple tags
  $ mononoke_admin bookmarks -R repo get tags/simple_tag
  Metadata changeset doesn't exist for tag bookmark tags/simple_tag
  Changeset pointed to by the tag bookmark tags/simple_tag
  da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c

# Change the value of changing_tag but keep it pointing to the same commit with the same name
  $ git tag -d changing_tag
  Deleted tag 'changing_tag' (was 5733f87)
  $ git tag -a -m "changing it again" changing_tag $tagged_commit

# Importing a second time should still work
  $ with_stripped_logs gitimport "$GIT_REPO" --generate-bookmarks full-repo
  using repo "repo" repoid RepositoryId(0)
  GitRepo:$TESTTMP/repo-git 2 of 2 commit(s) already exist* (glob)
  Ref: "refs/heads/master_bookmark": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Ref: "refs/remotes/origin/a_ref_prefixed_by_remotes_origin": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/changing_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/empty_tag": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Ref: "refs/tags/first_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/recursive_tag": Some(ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)))
  Ref: "refs/tags/simple_tag": Some(ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)))
  Initializing repo: repo
  Initialized repo: repo
  All repos initialized. It took: 0 seconds
  Bookmark: "heads/master_bookmark": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (already up-to-date)
  Bookmark: "remotes/origin/a_ref_prefixed_by_remotes_origin": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "tags/changing_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "tags/empty_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (already up-to-date)
  Bookmark: "tags/first_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "tags/recursive_tag": ChangesetId(Blake2(032cd4dce0406f1c1dd1362b6c3c9f9bdfa82f2fc5615e237a890be4fe08b044)) (already up-to-date)
  Bookmark: "tags/simple_tag": ChangesetId(Blake2(da93dc81badd8d407db0f3219ec0ec78f1ef750ebfa95735bb483310371af80c)) (already up-to-date)

# Generating bookmarks should also capture the tag mapping in bonsai_tag_mapping table. Note that changing_tag has changed in hash
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name, hex(changeset_id) as cs_id, hex(tag_hash) as tag_hash, target_is_tag FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/changing_tag|3EA206D29D695D32C490FF0B2E8136B7ACF63783467515DB9C5DDB86122AD025|E73C25830B102663DB6F17C054C0EE3F9A93C04A|0
  tags/empty_tag|1910A71753B6A3F0A308C44E85AE28EB57272D5519D53C4577AF4395784EFDB3|FB02ED046A1E75FE2ABB8763F7C715496AE36353|0
  tags/first_tag|5CA579C0E3EBEA708371B65CE559E5A51B231AD1B6F3CDFD874CA27362A2A6A8|8963E1F55D1346A07C3AEC8C8FC72BF87D0452B1|0
  tags/recursive_tag|1AB4E7C855BE1F10B2A3E48A398B7B068EFB96EE81A75B8F74654B521D28A988|18B57EB6E2869701C04EE36399FCDE1A824A00DD|1

# Start Mononoke
  $ start_and_wait_for_mononoke_server
# Clone the repository
  $ cd "$TESTTMP"
  $ setconfig remotenames.selectivepulldefault=master_bookmark,heads/master_bookmark
  $ hg clone -q mono:repo "$HG_REPO"
  $ cd "$HG_REPO"
  $ cat "file1"
  this is file1
  $ cat "file2"
  this is file2

# Check that we can see the git hash from extras
  $ hg log --config extensions.gitrevset= --template 'hg={node}: git={gitnode}\nextras=(\n{extras % "  {extra}\n"})\n' -r heads/master_bookmark
  hg=e7f52161c6127445391295b677f87aded035450a: git=e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
  extras=(
    branch=default
    convert_revision=e8615d6f149b876be0a2f30a1c5bf0c42bf8e136
    hg-git-rename-source=git
  )

# Checks all the bookmarks were created
  $ hg bookmarks --all --remote
     remote/heads/master_bookmark     e7f52161c6127445391295b677f87aded035450a
     remote/remotes/origin/a_ref_prefixed_by_remotes_origin b48ed460078564067eabec6c4a50909d000d7e22
     remote/tags/changing_tag         b48ed460078564067eabec6c4a50909d000d7e22
     remote/tags/empty_tag            e7f52161c6127445391295b677f87aded035450a
     remote/tags/first_tag            b48ed460078564067eabec6c4a50909d000d7e22
     remote/tags/recursive_tag        b48ed460078564067eabec6c4a50909d000d7e22
     remote/tags/simple_tag           e7f52161c6127445391295b677f87aded035450a
