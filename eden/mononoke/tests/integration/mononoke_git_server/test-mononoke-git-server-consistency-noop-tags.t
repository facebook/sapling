# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
# Disable tag-mapping caching to check database write order, not independent tag-cache freshness.
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:git_atomic_tag_mapping": true,
  >     "scm/mononoke:enable_bonsai_tag_mapping_caching": false
  >   }
  > }
  > EOF

# Import a lightweight tag and warm its publishing bookmark through a real clone.
  $ GIT_REPO_ORIGIN="$TESTTMP/origin/repo-git"
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo content > file
  $ git add file
  $ git commit -qm initial
  $ git tag retyped
  $ cd "$TESTTMP"
  $ quiet git clone --mirror "$GIT_REPO_ORIGIN" repo-git
  $ quiet gitimport "$TESTTMP/repo-git" --derive-hg --generate-bookmarks full-repo
  $ set_mononoke_as_source_of_truth_for_git
  $ mononoke_git_service
  $ quiet git_client clone "$MONONOKE_GIT_SERVICE_BASE_URL/$REPONAME.git" client
  $ cd client

# Bound requests to the local test server if the non-yielding wait regresses.
  $ git config http.lowSpeedLimit 1
  $ git config http.lowSpeedTime 60
  $ git config http.extraHeader 'x-git-read-after-write-consistency: 1'
# SQLite stores these names as BLOBs; TEXT equality would silently match no rows.
  $ original_bonsai=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(changeset_id) FROM bookmarks WHERE repo_id=$REPOID AND name=CAST('tags/retyped' AS BLOB)")
  $ test -n "$original_bonsai"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bonsai_tag_mapping WHERE repo_id=$REPOID AND tag_name=CAST('tags/retyped' AS BLOB)"
  0
  $ git rev-parse refs/tags/retyped > "$TESTTMP/expected_tag"
  $ git_client ls-remote --refs origin refs/tags/retyped | cut -f1 > "$TESTTMP/advertised_tag"
  $ diff "$TESTTMP/expected_tag" "$TESTTMP/advertised_tag"

# Lightweight -> annotated at the same commit is a Bonsai no-op, but native Git
# sends the changed tag OID. The direct mapping write must complete before success.
# The reverse conversion is omitted: write advertisement peels tags, so Git elides it.
  $ git tag -f -a -m 'annotated tag' retyped HEAD >/dev/null
  $ git_client push --force origin refs/tags/retyped > "$TESTTMP/annotate-push.log" 2>&1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bonsai_tag_mapping WHERE repo_id=$REPOID AND tag_name=CAST('tags/retyped' AS BLOB)"
  1
  $ test "$original_bonsai" = "$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(changeset_id) FROM bookmarks WHERE repo_id=$REPOID AND name=CAST('tags/retyped' AS BLOB)")"
  $ git rev-parse refs/tags/retyped > "$TESTTMP/expected_tag"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT lower(hex(tag_hash)) FROM bonsai_tag_mapping WHERE repo_id=$REPOID AND tag_name=CAST('tags/retyped' AS BLOB)" > "$TESTTMP/stored_tag"
  $ diff "$TESTTMP/expected_tag" "$TESTTMP/stored_tag"
  $ git_client ls-remote --refs origin refs/tags/retyped | cut -f1 > "$TESTTMP/advertised_tag"
  $ diff "$TESTTMP/expected_tag" "$TESTTMP/advertised_tag"

# Replacing only an annotation is also a Bonsai no-op, not a Git OID no-op.
  $ previous_tag=$(git rev-parse refs/tags/retyped)
  $ git tag -f -a -m 'replacement annotation' retyped HEAD >/dev/null
  $ test "$previous_tag" != "$(git rev-parse refs/tags/retyped)"
  $ git_client push --force origin refs/tags/retyped > "$TESTTMP/replace-annotation-push.log" 2>&1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bonsai_tag_mapping WHERE repo_id=$REPOID AND tag_name=CAST('tags/retyped' AS BLOB)"
  1
  $ test "$original_bonsai" = "$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT hex(changeset_id) FROM bookmarks WHERE repo_id=$REPOID AND name=CAST('tags/retyped' AS BLOB)")"
  $ git rev-parse refs/tags/retyped > "$TESTTMP/expected_tag"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT lower(hex(tag_hash)) FROM bonsai_tag_mapping WHERE repo_id=$REPOID AND tag_name=CAST('tags/retyped' AS BLOB)" > "$TESTTMP/stored_tag"
  $ diff "$TESTTMP/expected_tag" "$TESTTMP/stored_tag"
  $ git_client ls-remote --refs origin refs/tags/retyped | cut -f1 > "$TESTTMP/advertised_tag"
  $ diff "$TESTTMP/expected_tag" "$TESTTMP/advertised_tag"
