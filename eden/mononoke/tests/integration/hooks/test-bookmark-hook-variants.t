# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config
  $ cd "$TESTTMP/mononoke-config"

# Configure bookmark hooks using all three methods: name, regex, and inverse_regex
  $ cat >> repos/repo/server.toml <<CONFIG
  > # Hook that applies only to the master bookmark (using name)
  > [[bookmarks]]
  > name="master"
  > [[bookmarks.hooks]]
  > hook_name="limit_filesize"
  > [[hooks]]
  > name="limit_filesize"
  > config_int_lists={filesize_limits_values=[5]}
  > config_string_lists={filesize_limits_regexes=[".*"]}
  > 
  > # Hook that applies to feature branches (using regex)
  > [[bookmarks]]
  > regex="^feature/.*"
  > [[bookmarks.hooks]]
  > hook_name="block_merge_commits"
  > [[hooks]]
  > name="block_merge_commits"
  > config_json='{"disable_merge_bypass": true}'
  > 
  > # Hook that applies to everything EXCEPT master and main branches (using inverse_regex)
  > [[bookmarks]]
  > inverse_regex="^(master|main)$"
  > [[bookmarks.hooks]]
  > hook_name="limit_commit_message_length"
  > [[hooks]]
  > name="limit_commit_message_length"
  > config_json='{"length_limit": 50}'
  > CONFIG

  $ cd $TESTTMP

  $ enable amend
  $ setconfig remotenames.selectivepulldefault=master

setup repo
  $ testtool_drawdag -R repo <<EOF
  > C
  > |
  > B
  > |
  > A
  > # bookmark: C master
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

start mononoke
  $ start_and_wait_for_mononoke_server

Clone the repo
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2

Test 1: Bookmark with specific name (master) - should only run limit_filesize hook
  $ hg up -q master
  $ echo 'tiny' > small_file
  $ hg add small_file
  $ hg ci -m 'Add small file to master'
  $ hg push -r . --to master -q

# Try pushing a large file to master - should fail due to limit_filesize hook
  $ echo 'this is too large' > large_file
  $ hg add large_file
  $ hg ci -m 'Add large file to master'
  $ hg push -r . --to master
  pushing rev * to destination mono:repo bookmark master (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_filesize for 8e1abc616627abff95c42a990ee914e7cb6a2bcf: File size limit is 5 bytes. You tried to push file large_file that is over the limit (18 bytes, 3.60x the limit). This limit is enforced for files matching the following regex: ".*". See https://fburl.com/landing_big_diffs for instructions.
  abort: unexpected EOL, expected netstring digit
  [255]

Test 2: Feature branch (matches regex) - should run block_merge_commits and limit_commit_message_length hooks
# Create a feature branch
  $ hg up -q master
  $ hg bookmark feature/test-branch

# Try to push a commit with a long message - should fail due to limit_commit_message_length (inverse_regex applies here)
  $ echo 'content' > feature_file
  $ hg add feature_file
  $ hg ci -m 'This commit message is definitely longer than fifty characters and should be rejected by the limit_commit_message_length hook'
  $ hg push -r . --to feature/test-branch --create
  pushing rev * to destination mono:repo bookmark feature/test-branch (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_message_length for *: Commit message length for '*' (125) exceeds length limit (>= 50) (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

# Try with a shorter commit message - should work
  $ hg amend -m 'Short message'
  $ hg push -r . --to feature/test-branch --create -q

# Now try to create a merge commit - should fail due to block_merge_commits hook
  $ hg up -q master
  $ echo 'x' > master_file
  $ hg add master_file
  $ hg ci -m 'Change on master'
  $ hg push -r . --to master -q

  $ hg up -q feature/test-branch
  $ hg merge master
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'Merge commit'
  $ hg push -r . --to feature/test-branch
  pushing rev * to destination mono:repo bookmark feature/test-branch (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_merge_commits for *: You must not commit merge commits (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

Test 3: Non-main branch that doesn't match regex - should only run limit_commit_message_length (inverse_regex)
# Create a development branch that doesn't match feature/* pattern
  $ hg up -q master
  $ hg bookmark dev-branch
  $ echo 'dev content' > dev_file
  $ hg add dev_file

# Try with long commit message - should fail
  $ hg ci -m 'This development branch commit message is also longer than fifty characters'
  $ hg push -r . --to dev-branch --create
  pushing rev * to destination mono:repo bookmark dev-branch (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_message_length for *: Commit message length for '*' (75) exceeds length limit (>= 50) (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

# Try with short commit message - should work
  $ hg amend -m 'Short dev message'
  $ hg push -r . --to dev-branch --create -q

# Large files should be allowed on dev-branch (no limit_filesize hook)
  $ echo 'this is a very large file content for dev branch' > large_dev_file
  $ hg add large_dev_file
  $ hg ci -m 'Large file on dev'
  $ hg push -r . --to dev-branch -q

# Merge commits should be allowed on dev-branch (no block_merge_commits hook)
  $ hg up -q master
  $ echo 'y' > master_file2
  $ hg add master_file2
  $ hg ci -m 'Another master change'
  $ hg push -r . --to master -q

  $ hg up -q dev-branch
  $ hg merge master
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'Dev merge'
  $ hg push -r . --to dev-branch -q

Test 4: Main branch (excluded by inverse_regex) - should only run hooks that apply to it specifically
# Test that main branch doesn't get the inverse_regex hook
  $ hg up -q master
  $ hg bookmark main
  $ echo 'main content' > main_file
  $ hg add main_file

# Long commit message should be allowed on main (inverse_regex excludes main)
  $ hg ci -m 'This is a very long commit message for the main branch that should be allowed because main is excluded by the inverse_regex pattern'
  $ hg push -r . --to main --create -q

# But large files should still be rejected if there was a limit_filesize hook for main
# (There isn't one in this test, so large files should be allowed)
  $ echo 'large content for main branch' > large_main_file
  $ hg add large_main_file
  $ hg ci -m 'Large file on main'
  $ hg push -r . --to main -q

Test 5: Verify hook combinations work correctly
# Test that a branch matching both regex and inverse_regex gets both sets of hooks
# Create feature/special branch that should get both feature/* hooks and inverse_regex hooks
  $ hg up -q master
  $ hg bookmark feature/special
  $ echo 'special content' > special_file
  $ hg add special_file

# Should fail with long message (inverse_regex hook)
  $ hg ci -m 'This feature branch commit message is longer than fifty characters'
  $ hg push -r . --to feature/special --create
  pushing rev * to destination mono:repo bookmark feature/special (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     limit_commit_message_length for *: Commit message length for '*' (66) exceeds length limit (>= 50) (glob)
  abort: unexpected EOL, expected netstring digit
  [255]

# Should work with short message
  $ hg amend -m 'Special feature'
  $ hg push -r . --to feature/special --create -q

# Should also fail with merge commit (regex hook)
  $ hg up -q master
  $ echo 'z' > master_merge_file
  $ hg add master_merge_file
  $ hg ci -m 'Master for merge'
  $ hg push -r . --to master -q

  $ hg up -q feature/special
  $ hg merge master
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'Feature merge'
  $ hg push -r . --to feature/special
  pushing rev * to destination mono:repo bookmark feature/special (glob)
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_merge_commits for *: You must not commit merge commits (glob)
  abort: unexpected EOL, expected netstring digit
  [255]
