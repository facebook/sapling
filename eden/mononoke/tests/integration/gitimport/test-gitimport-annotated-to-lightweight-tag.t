# (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config blob_files
  $ GIT_REPO="$TESTTMP/repo-git"
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo a > a
  $ git add a
  $ git commit -qm a
  $ A=$(git rev-parse HEAD)
  $ echo b > b
  $ git add b
  $ git commit -qm b
  $ B=$(git rev-parse HEAD)
  $ git tag -a -m moved moved "$A"
  $ git tag -a -m kept kept "$A"
  $ cd "$TESTTMP"
  $ function import() { gitimport "$GIT_REPO" --generate-bookmarks --suppress-ref-mapping --bypass-non-fast-forward --cleanup-mononoke-bookmarks full-repo > /dev/null 2> "$TESTTMP/import.log" || { cat "$TESTTMP/import.log"; return 1; }; }
  $ function tag_mappings() { sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT CAST(tag_name AS TEXT) FROM bonsai_tag_mapping ORDER BY tag_name"; }
  $ function bookmark_is() { test "$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT lower(hex(g.git_sha1)) FROM bookmarks b JOIN bonsai_git_mapping g ON b.changeset_id=g.bcs_id AND b.repo_id=g.repo_id WHERE CAST(b.name AS TEXT)='$1'")" = "$2"; }

Both annotated tags get a tag mapping row.

  $ import
  $ tag_mappings
  tags/kept
  tags/moved

Both tags are replaced by lightweight tags: one on a new commit, one on the same
commit, so its bookmark does not move. Both stale rows go, otherwise the git
server keeps advertising the old tag objects.

  $ git -C "$GIT_REPO" tag -f moved "$B" > /dev/null
  $ git -C "$GIT_REPO" tag -f kept "$A" > /dev/null
  $ import
  $ tag_mappings
  $ bookmark_is tags/moved "$B"
  $ bookmark_is tags/kept "$A"

An annotated tag created again under the same name has a row.

  $ git -C "$GIT_REPO" tag -f -a -m again kept "$B" > /dev/null
  $ import
  $ tag_mappings
  tags/kept
  $ bookmark_is tags/kept "$B"
