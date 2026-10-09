# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config blob_files
  $ GIT_REPO="$TESTTMP/repo-git"
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo base > base
  $ git add base
  $ git commit -qm base
  $ BASE=$(git rev-parse HEAD)
  $ git branch release
  $ git checkout -qb unmanaged
  $ echo unmanaged > unmanaged
  $ git add unmanaged
  $ git commit -qm unmanaged
  $ UNMANAGED=$(git rev-parse HEAD)
  $ git checkout -q master_bookmark
  $ cd "$TESTTMP"
  $ function incremental() { gitimport "$GIT_REPO" --generate-bookmarks --suppress-ref-mapping --bypass-non-fast-forward --include-refs "$1" "${@:2}" incremental > "$TESTTMP/phases.log" 2> "$TESTTMP/import.log" || { cat "$TESTTMP/import.log"; return 1; }; }
  $ function bookmark_is() { test "$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT lower(hex(g.git_sha1)) FROM bookmarks b JOIN bonsai_git_mapping g ON b.changeset_id=g.bcs_id AND b.repo_id=g.repo_id WHERE CAST(b.name AS TEXT)='$1'")" = "$2"; }
  $ function mapping_count() { sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bonsai_git_mapping"; }
  $ SELECTED=refs/heads/master_bookmark,refs/heads/release

Only selected histories are imported; unrelated Git tips and Mononoke bookmarks survive.

  $ incremental "$SELECTED"
  $ test ! -s "$TESTTMP/phases.log"
  $ mapping_count
  1
  $ bookmark_is heads/master_bookmark "$BASE"
  $ bookmark_is heads/release "$BASE"
  $ mononoke_admin bookmarks -R repo get heads/unmanaged
  (not set)
  $ mononoke_admin bookmarks -R repo set heads/unmanaged "git=$BASE" > /dev/null

An ordinary advance imports its new commit and preserves the other selected head.

  $ cd "$GIT_REPO"
  $ echo main > main
  $ git add main
  $ git commit -qm main
  $ MAIN=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ incremental "$SELECTED"
  $ mapping_count
  2
  $ bookmark_is heads/master_bookmark "$MAIN"
  $ bookmark_is heads/release "$BASE"

The union includes a separate release tip and an unselected merge parent's missing history.

  $ cd "$GIT_REPO"
  $ git checkout -q release
  $ echo release > release
  $ git add release
  $ git commit -qm release
  $ RELEASE=$(git rev-parse HEAD)
  $ git checkout -qb topic "$BASE"
  $ echo topic > topic
  $ git add topic
  $ git commit -qm topic
  $ TOPIC=$(git rev-parse HEAD)
  $ git checkout -q master_bookmark
  $ git merge -q --no-ff -m merge topic
  $ MERGE=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ incremental "$SELECTED" --log-import-phases
  $ mapping_count
  5
  $ bookmark_is heads/master_bookmark "$MERGE"
  $ bookmark_is heads/release "$RELEASE"
  $ bookmark_is heads/unmanaged "$BASE"
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bonsai_git_mapping WHERE lower(hex(git_sha1))='$TOPIC'"
  1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bonsai_git_mapping WHERE lower(hex(git_sha1))='$UNMANAGED'"
  0
  $ sed -n 's/^gitimport_phase phase=\([^ ]*\) duration_ms=[0-9]*$/\1/p' "$TESTTMP/phases.log"
  main_entered
  startup
  open_repo
  discover_commits
  import_contents
  resolve_refs
  open_managed_repo
  publication
  async_cleanup
  runtime_shutdown
  $ head -1 "$TESTTMP/phases.log"
  gitimport_phase phase=main_entered duration_ms=0
  $ grep -c gitimport_phase "$TESTTMP/import.log"
  0
  [1]

An already-mapped no-op has no new bookmark log entries.

  $ BEFORE=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bookmarks_update_log")
  $ incremental "$SELECTED"
  $ test "$BEFORE" = "$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bookmarks_update_log")"
  $ grep 'Nothing to import' "$TESTTMP/import.log"
  [INFO] Nothing to import for repo $TESTTMP/repo-git.

Mapped targets must still publish on force rewind and repair destination drift.

  $ git -C "$GIT_REPO" update-ref refs/heads/master_bookmark "$BASE"
  $ incremental "$SELECTED"
  $ bookmark_is heads/master_bookmark "$BASE"
  $ mononoke_admin bookmarks -R repo set heads/master_bookmark "git=$MAIN" > /dev/null
  $ incremental "$SELECTED"
  $ bookmark_is heads/master_bookmark "$BASE"
  $ bookmark_is heads/release "$RELEASE"
  $ bookmark_is heads/unmanaged "$BASE"
  $ mapping_count
  5

Annotated tags are peeled and published even when their target is already mapped.

  $ git -C "$GIT_REPO" tag -a -m selected selected "$BASE"
  $ incremental "$SELECTED,refs/tags/selected"
  $ BASE_BONSAI=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT lower(hex(bcs_id)) FROM bonsai_git_mapping WHERE lower(hex(git_sha1))='$BASE'")
  $ test "$(mononoke_admin bookmarks -R repo get tags/selected | tail -1)" = "$BASE_BONSAI"

A missing selected ref fails before importing any new content or publishing a bookmark.

  $ cd "$GIT_REPO"
  $ echo pending > pending
  $ git add pending
  $ git commit -qm pending
  $ PENDING=$(git rev-parse HEAD)
  $ cd "$TESTTMP"
  $ BEFORE=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bookmarks_update_log")
  $ gitimport "$GIT_REPO" --generate-bookmarks --include-refs refs/heads/master_bookmark,refs/heads/missing incremental > "$TESTTMP/missing.log" 2>&1
  [1]
  $ grep -c 'Selected ref does not exist: refs/heads/missing' "$TESTTMP/missing.log"
  1
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bonsai_git_mapping WHERE lower(hex(git_sha1))='$PENDING'"
  0
  $ test "$BEFORE" = "$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bookmarks_update_log")"
  $ bookmark_is heads/master_bookmark "$BASE"

Content refs and unscoped cleanup are explicitly unsupported by incremental mode.

  $ TREE=$(git -C "$GIT_REPO" rev-parse HEAD^{tree})
  $ git -C "$GIT_REPO" update-ref refs/trees/selected "$TREE"
  $ gitimport "$GIT_REPO" --allow-content-refs --generate-bookmarks --include-refs refs/trees/selected incremental > "$TESTTMP/content.log" 2>&1
  [1]
  $ grep -c 'Incremental import does not support content ref refs/trees/selected' "$TESTTMP/content.log"
  1
  $ gitimport "$GIT_REPO" --generate-bookmarks --include-refs "$SELECTED" --cleanup-mononoke-bookmarks incremental > "$TESTTMP/cleanup.log" 2>&1
  [1]
  $ grep -c 'Incremental import does not support bookmark cleanup or reupload' "$TESTTMP/cleanup.log"
  1
  $ test "$BEFORE" = "$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT count(*) FROM bookmarks_update_log")"
