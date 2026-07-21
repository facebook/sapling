# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

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

# Setup git repository
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
# Create a tag pointing to the first commit
  $ git tag -a -m "new tag" first_tag
  $ current_first_tag=$(git rev-parse HEAD)
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"
# Create another tag pointing to the second commit
  $ git tag -a -m "second tag" second_tag

  $ cd "$TESTTMP"
  $ git clone "$GIT_REPO_ORIGIN"
  Cloning into 'repo-git'...
  done.

# Enable logging of bookmark updates
  $ mkdir -p $TESTTMP/scribe_logs
  $ touch $TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY

# Import it into Mononoke
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo

# Set Mononoke as the Source of Truth
  $ set_mononoke_as_source_of_truth_for_git

# Both annotated tags have a bonsai_tag_mapping entry and a bookmark
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/first_tag
  tags/second_tag

# Start up the Mononoke Git Service
  $ mononoke_git_service
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git
  $ cd repo
  $ git show-ref | grep tags | sort
  8963e1f55d1346a07c3aec8c8fc72bf87d0452b1 refs/tags/first_tag
  ce5a26ba55c422e8e3960224153ef5cf35e75b14 refs/tags/second_tag

# Convert first_tag from an annotated tag to a lightweight tag pointing at a new commit
  $ echo "this is file3" > file3
  $ git add file3
  $ git commit -qam "Add file3"
  $ git tag -f first_tag
  Updated tag 'first_tag' (was 8963e1f)

# Push the updated (now lightweight) tag to the server
  $ git_client push origin --force HEAD:refs/tags/first_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   + 8ce3eae...bfc616e HEAD -> first_tag (forced update)

# The push commits the DB synchronously: the stale annotated first_tag
# bonsai_tag_mapping row is deleted in the same transaction as the bookmark move,
# leaving only second_tag (contrast S520024 / overridden-tags.t). No wait needed
# for these direct DB reads.
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/second_tag
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name FROM bookmarks WHERE name LIKE 'tags/%' ORDER BY name"
  tags/first_tag
  tags/second_tag

# The annotated->lightweight conversion settles asynchronously in the read path
# (the WBC and the bonsai_tag_mapping cache refresh independently), so the served
# value transitions through the old peeled commit before settling on the new
# lightweight target. Poll until Mononoke advertises the FINAL value before
# cloning -- waiting only for "moved off the old value" (wait_for_git_bookmark_move)
# can catch the transient and make the clone flaky.
  $ new_first_tag=$(git rev-parse HEAD)
  $ attempt=0
  $ while [ "$(git_client ls-remote --quiet 2>/dev/null | awk '$2 == "refs/tags/first_tag" {print $1}')" != "$new_first_tag" ] && [ "$attempt" -lt 120 ]; do
  >   sleep 1
  >   attempt=$((attempt + 1))
  > done

# A fresh all-refs clone succeeds and first_tag now points at the new commit (no
# "did not receive expected object")
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo
  $ cd new_repo
  $ git show-ref | grep tags | sort
  bfc616ee3980b461c7d38caa901fa591aa776631 refs/tags/first_tag
  ce5a26ba55c422e8e3960224153ef5cf35e75b14 refs/tags/second_tag

# --- Write-hook path: push a brand-new annotated tag through the server ---
# This drives tag_mapping_write_hook (the core fix), which the conversion above
# does not exercise. The mapping row must be written inside the bookmark txn.
  $ cd "$TESTTMP/repo"
  $ git tag -a -m "third tag" third_tag
  $ git_client push origin refs/tags/third_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new tag]         third_tag -> third_tag
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/second_tag
  tags/third_tag
# Poll until Mononoke advertises the annotated tag object before cloning (the ref
# can appear in the WBC before the bonsai_tag_mapping cache has the tag object).
  $ expected_third=$(git rev-parse refs/tags/third_tag)
  $ attempt=0
  $ while [ "$(git_client ls-remote --quiet 2>/dev/null | awk '$2 == "refs/tags/third_tag" {print $1}')" != "$expected_third" ] && [ "$attempt" -lt 120 ]; do
  >   sleep 1
  >   attempt=$((attempt + 1))
  > done
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo_third
  $ cd new_repo_third
  $ git show-ref | grep third_tag
  be113a7a3bb74c0e5697be441011037a71c5a33e refs/tags/third_tag

# --- Nested annotated tag: the inner tag is in the pack but not pushed as its
# own ref, so its mapping must still be written (inline) even in atomic mode ---
  $ cd "$TESTTMP/repo"
  $ git config advice.nestedTag false
  $ git tag -a -m "inner" inner_tag HEAD
  $ git tag -a -m "outer" outer_tag inner_tag
  $ git_client push origin refs/tags/outer_tag
  To https://localhost:$LOCAL_PORT/repos/git/ro/repo.git
   * [new tag]         outer_tag -> outer_tag
# Both the referenced outer tag AND the unreferenced inner tag get a mapping row
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  inner_tag
  tags/outer_tag
  tags/second_tag
  tags/third_tag
# Poll until Mononoke advertises the outer annotated tag object before cloning.
  $ expected_outer=$(git rev-parse refs/tags/outer_tag)
  $ attempt=0
  $ while [ "$(git_client ls-remote --quiet 2>/dev/null | awk '$2 == "refs/tags/outer_tag" {print $1}')" != "$expected_outer" ] && [ "$attempt" -lt 120 ]; do
  >   sleep 1
  >   attempt=$((attempt + 1))
  > done
# A clone of the nested tag succeeds (inner tag object is advertised and packed)
  $ cd "$TESTTMP"
  $ quiet git_client clone $MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git new_repo_nested
  $ cd new_repo_nested
  $ git show-ref | grep -E "outer_tag|inner_tag" | sort
  9ed3e91d3db3250a4c8518b32fd6b61fc72c620b refs/tags/outer_tag
