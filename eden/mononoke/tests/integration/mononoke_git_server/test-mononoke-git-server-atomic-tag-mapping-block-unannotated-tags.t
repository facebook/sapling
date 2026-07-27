# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Regression guard: with git_atomic_tag_mapping ON, block_unannotated_tags must trust the server's annotated-tag signal (mapping row is written in the bookmark txn, after hooks run).

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ export WBC_SCRIBE_CATEGORY=mononoke_bookmark
  $ export TAGS_SCRIBE_CATEGORY=mononoke_bookmark
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ . "${TEST_FIXTURES}/library.sh"
  $ export ENABLE_BOOKMARK_CACHE=1
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:wbc_update_by_scribe_tailer": true,
  >     "scm/mononoke:enable_bonsai_tag_mapping_caching": true,
  >     "scm/mononoke:git_atomic_tag_mapping": true
  >   }
  > }
  > EOF

  $ cat >> repos/repo/server.toml <<EOF
  > [[bookmarks]]
  > regex=".*"
  > [[bookmarks.hooks]]
  > hook_name="block_unannotated_tags"
  > [[hooks]]
  > name="block_unannotated_tags"
  > config_json='{}'
  > EOF

# Setup git repository with a baseline annotated tag
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m "new tag" first_tag
  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

  $ mkdir -p $TESTTMP/scribe_logs
  $ touch $TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/first_tag

# Start up the Mononoke Git Service and clone
  $ mononoke_git_service
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ cd repo

# (a) A brand-new annotated tag must succeed (rejected before the fix)
  $ git tag -a -m "annotated" annotated_tag
  $ git_client push origin annotated_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new tag]         annotated_tag -> annotated_tag

# The atomic path wrote the mapping row synchronously in the bookmark txn
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/annotated_tag
  tags/first_tag

# (b) A lightweight tag is never in the trusted set, so it is still rejected
  $ git tag lightweight_tag
  $ git_client push origin lightweight_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] lightweight_tag -> lightweight_tag (hooks failed:
    block_unannotated_tags for 8ce3eae44760b500bf3f2c3922a95dcd3c908e9e: The un-annotated tag "tags/lightweight_tag" is not allowed in this repository.
  Use 'git tag [ -a | -s ]' for tags you want to propagate.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]


  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/annotated_tag
  tags/first_tag

# (c) Atomic multi-ref push of TWO fresh annotated tags: annotated_tags is built from both ops of one set_refs batch and both are accepted.
  $ git tag -a -m "atomic one" atomic_annotated_one
  $ git tag -a -m "atomic two" atomic_annotated_two
  $ git_client push --atomic origin atomic_annotated_one atomic_annotated_two
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new tag]         atomic_annotated_one -> atomic_annotated_one
   * [new tag]         atomic_annotated_two -> atomic_annotated_two
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/annotated_tag
  tags/atomic_annotated_one
  tags/atomic_annotated_two
  tags/first_tag

# (d) Atomic push of a fresh annotated + fresh lightweight tag in one set_refs batch: the annotated tag is accepted via the shared set while the lightweight is rejected, so the atomic push fails and neither ref is written.
  $ git tag -a -m "mixed annotated" atomic_mixed_annotated
  $ git tag atomic_mixed_lightweight
  $ git_client push --atomic origin atomic_mixed_annotated atomic_mixed_lightweight
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   ! [remote rejected] atomic_mixed_annotated -> atomic_mixed_annotated (Atomic bookmark update failed with error: hooks failed:
    block_unannotated_tags for 609d5d5ebbd78ff05c51516587dff147fa426f79: The un-annotated tag "tags/atomic_mixed_lightweight" is not allowed in this repository.
  Use 'git tag [ -a | -s ]' for tags you want to propagate.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
   ! [remote rejected] atomic_mixed_lightweight -> atomic_mixed_lightweight (Atomic bookmark update failed with error: hooks failed:
    block_unannotated_tags for 609d5d5ebbd78ff05c51516587dff147fa426f79: The un-annotated tag "tags/atomic_mixed_lightweight" is not allowed in this repository.
  Use 'git tag [ -a | -s ]' for tags you want to propagate.
  
  For more information about hooks and bypassing, refer https://fburl.com/wiki/mb4wtk1j)
  error: failed to push some refs to 'https://localhost:$LOCAL_PORT/repos/git/ro/repo.git'
  [1]

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/annotated_tag
  tags/atomic_annotated_one
  tags/atomic_annotated_two
  tags/first_tag
