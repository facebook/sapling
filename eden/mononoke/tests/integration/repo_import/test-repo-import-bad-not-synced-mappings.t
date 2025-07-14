# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test that repo_import ensures that when importing into a large repo, the mapping
# provided exists and is indeed a large-repo only mapping.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup configuration
  $ setup_configerator_configs
-- Init Mononoke thingies
  $ init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

-- Setup git repository
  $ cd "$TESTTMP"
  $ export GIT_REPO=git_repo
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "file1" > file1
  $ git add file1
  $ git commit -aqm "Add file1"
  $ echo "file1a" > file1
  $ git commit -aqm "Modify file1 A"
  $ echo "file1b" > file1
  $ git commit -aqm "Modify file1 B"

-- Try to import passing a mapping that does not exist
-- SHOULD FAIL
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDLARGE"
  $ repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "imported" \
  > --batch-size 3 \
  > --bookmark-suffix "imported" \
  > --disable-phabricator-check \
  > --dest-bookmark master_bookmark \
  > --git-merge-rev-id master_bookmark \
  > --commit-author user \
  > --commit-message "merging" \
  > --commit-date-rfc3339 "1970-01-01T00:00:00Z" \
  > --mark-not-synced-mapping "non_existent_version" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json"
  [INFO] using repo "large-mon" repoid RepositoryId(0)
  [ERROR] Execution error: Couldn't find commit sync config version non_existent_version
  Error: Execution failed
  [1]


-- Try to import passing a mapping that is not large-repo only
-- SHOULD FAIL
  $ repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "imported2" \
  > --batch-size 3 \
  > --bookmark-suffix "imported" \
  > --disable-phabricator-check \
  > --dest-bookmark master_bookmark \
  > --git-merge-rev-id master_bookmark \
  > --commit-author user \
  > --commit-message "merging again" \
  > --commit-date-rfc3339 "1970-01-01T00:00:00Z" \
  > --mark-not-synced-mapping "test_version" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json"
  [INFO] using repo "large-mon" repoid RepositoryId(0)
  [ERROR] Execution error: The provided mapping test_version is not a large-only mapping
  Error: Execution failed
  [1]
