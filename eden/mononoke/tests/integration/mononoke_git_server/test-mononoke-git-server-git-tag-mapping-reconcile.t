# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ GIT_REPO_ORIGIN="${TESTTMP}/origin/repo-git"
  $ GIT_REPO="${TESTTMP}/repo-git"

# Setup a git repo: commit1 carries an annotated tag; commit2 is a later commit
# we will (wrongly) point the tag bookmark at to simulate the S687348 divergence.
  $ mkdir -p "$GIT_REPO_ORIGIN"
  $ cd "$GIT_REPO_ORIGIN"
  $ git init -q
  $ echo "this is file1" > file1
  $ git add file1
  $ git commit -qam "Add file1"
  $ git tag -a -m "annotated tag" first_tag
  $ echo "this is file2" > file2
  $ git add file2
  $ git commit -qam "Add file2"

  $ cd "$TESTTMP"
  $ git clone -q "$GIT_REPO_ORIGIN"

# Capture the git commit shas: C1 is the commit the annotated tag points at,
# C2 is the (wrong) commit we will divert the bookmark to.
  $ cd "$GIT_REPO"
  $ C1=$(git rev-parse first_tag^{})
  $ C2=$(git rev-parse HEAD)

# Import into Mononoke and make it the source of truth.
  $ cd "$TESTTMP"
  $ quiet gitimport "$GIT_REPO" --derive-hg --generate-bookmarks full-repo
  $ set_mononoke_as_source_of_truth_for_git

# Bonsai of the tags/first_tag bookmark (awk-extracted to avoid nested-quote
# issues in the test shell's command substitution).
  $ X_BONSAI=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name, hex(changeset_id) FROM bookmarks" | awk -F"|" '$1=="tags/first_tag"{print $2}')

# Healthy state: the tags/first_tag bookmark points at the annotated tag's
# target, so reconcile finds nothing to do.
  $ mononoke_admin git-tag-mapping -R repo reconcile
  No diverged bonsai_tag_mapping rows found.

# Introduce a divergence: move the bookmark to C2 while leaving the annotated tag
# object (and its bonsai_tag_mapping row) pointing at C1.
  $ mononoke_admin bookmarks -R repo set tags/first_tag git=$C2 > /dev/null
  $ Y_BONSAI=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name, hex(changeset_id) FROM bookmarks" | awk -F"|" '$1=="tags/first_tag"{print $2}')
  $ [ -n "$X_BONSAI" ] && [ "$X_BONSAI" != "$Y_BONSAI" ] && echo "diverged"
  diverged

# Dry-run reports the recovery move (bookmark C2 -> C1) and changes nothing.
  $ mononoke_admin git-tag-mapping -R repo reconcile > dryrun.out 2>&1
  $ grep -q "move bookmark $C2 -> $C1" dryrun.out && echo "dry-run reports move C2 -> C1"
  dry-run reports move C2 -> C1
  $ grep -q "dry-run" dryrun.out && echo "did not apply"
  did not apply
  $ NOW=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name, hex(changeset_id) FROM bookmarks" | awk -F"|" '$1=="tags/first_tag"{print $2}')
  $ [ "$NOW" = "$Y_BONSAI" ] && echo "bookmark untouched by dry-run"
  bookmark untouched by dry-run

# Apply: the bookmark is moved back to the annotated tag's target (C1),
# recovering the annotated tag rather than downgrading it to a lightweight tag.
  $ mononoke_admin git-tag-mapping -R repo reconcile --apply
  DIVERGED tags/first_tag: move bookmark * -> * (glob)
  Recovered 1 annotated tag(s) by moving the bookmark to the tag target.

# The bookmark again matches the annotated tag's target...
  $ NOW=$(sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT name, hex(changeset_id) FROM bookmarks" | awk -F"|" '$1=="tags/first_tag"{print $2}')
  $ [ "$NOW" = "$X_BONSAI" ] && echo "recovered to tag target"
  recovered to tag target
# ...and the tag is still annotated: its bonsai_tag_mapping row is preserved
# (contrast the old behavior, which deleted the row / made the tag lightweight).
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "SELECT tag_name FROM bonsai_tag_mapping ORDER BY tag_name"
  tags/first_tag

# reconcile is a no-op again once everything is consistent.
  $ mononoke_admin git-tag-mapping -R repo reconcile
  No diverged bonsai_tag_mapping rows found.
