# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ export COMMIT_SCRIBE_CATEGORY=mononoke_commits
  $ export BOOKMARK_SCRIBE_CATEGORY=mononoke_bookmark
  $ export MONONOKE_TEST_SCRIBE_LOGGING_DIRECTORY=$TESTTMP/scribe_logs/
  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setconfig push.edenapi=true
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP

setup repo

  $ testtool_drawdag --print-hg-hashes -R repo --derive-all --no-default-files <<EOF
  > A
  > # modify: A "a" "a file content\n"
  > # bookmark: A master_bookmark
  > # message: A "a"
  > EOF
  A=325f1a90ab08ff67e563266a259738c9bd04284d

start mononoke

  $ start_and_wait_for_mononoke_server

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hg clone -q mono:repo repo2
  $ hg clone -q mono:repo repo3
  $ cd repo2
  $ hg pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo


create new commits in repo2 and check that they are seen as outgoing

  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb
  $ hg push -r . --to master_bookmark --create --config extensions.pushrebase=
  pushing rev 071266624f86 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (325f1a90ab08, 071266624f86] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 071266624f86

  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .repo_id
  0
  0
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  "repo"
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .bookmark
  null
  "master_bookmark"
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .generation
  2
  2
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .changeset_id
  "4a1bfca467c5d3861ae8d5788686650dc0afffbf6bc8fbe32887a59522c30cf0"
  "4a1bfca467c5d3861ae8d5788686650dc0afffbf6bc8fbe32887a59522c30cf0"
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .bubble_id
  null
  null
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .parents
  [
    "1482ddeb2a1515808f6e8aa50b06a429ecc778f66135f25d57c355823b1e9b4c"
  ]
  [
    "1482ddeb2a1515808f6e8aa50b06a429ecc778f66135f25d57c355823b1e9b4c"
  ]
Note: user_unix_name, user_identities and source_hostname are different between oss and fb context, so only test them in the facebook directory
The timestamp is not stable, so count its digits instead to ensure it is not null
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .received_timestamp | { IFS= read -r timestamp; printf '%s\n' "${#timestamp}"; }
  10
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .changed_files_count
  2
  2
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .changed_files_size
  34
  34
  $ rm "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY"

  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_name
  "master_bookmark"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_kind
  "publishing"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .old_bookmark_value
  "1482ddeb2a1515808f6e8aa50b06a429ecc778f66135f25d57c355823b1e9b4c"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  "4a1bfca467c5d3861ae8d5788686650dc0afffbf6bc8fbe32887a59522c30cf0"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "pushrebase"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "pushrebase"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"

  $ echo forcepushrebase > forcepushrebase
  $ hg add -q forcepushrebase
  $ hg ci -m forcepushrebase
  $ hg push -r . --to forcepushrebase --create --force --config extensions.pushrebase=
  pushing rev a5b0fe1646b4 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark forcepushrebase
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  creating remote bookmark forcepushrebase
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .bookmark
  null
  "forcepushrebase"
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .changeset_id
  "efb773fb49e1ebb720e998299840f573cd569c54ad96c0ad39027d4d7efbb447"
  "efb773fb49e1ebb720e998299840f573cd569c54ad96c0ad39027d4d7efbb447"
  $ rm "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY"

  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_name
  "forcepushrebase"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_kind
  "publishing"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .old_bookmark_value
  null
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  "efb773fb49e1ebb720e998299840f573cd569c54ad96c0ad39027d4d7efbb447"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "create"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "apirequest"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"

Use normal push (non-pushrebase).  Since we are not pushing to a public bookmark, this is draft.
  $ echo push > push
  $ hg add -q push
  $ hg ci -m 'commit'
  $ hg push --force --allow-anon
  pushing to mono:repo
  searching for changes

  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq 'select(.is_public == false)' | jq .bookmark
  null
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq 'select(.is_public == false)' | jq .changeset_id
  "e2af88a3e2d349c9848c019d347f0210acb640bc2282cd8fcce48ac452de5beb"
  $ rm "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY"

Use infinitepush push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitcloud=
  > [infinitepush]
  > server=False
  > branchpattern=re:^scratch/.+$
  > EOF

Stop tracking master_bookmark
  $ hg up -q $A
  $ echo infinitepush > infinitepush
  $ hg add -q infinitepush
  $ hg ci -m 'infinitepush'
  $ hg push -qr . --to "scratch/123" --create
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq 'select(.is_public == false)' | jq .changeset_id
  "650cba20f2f6bb385ff6fe14c21e04f17da2ef121420f36a441c2187e168fd80"

  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_name
  "scratch/123"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_kind
  "scratch"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .old_bookmark_value
  null
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  "650cba20f2f6bb385ff6fe14c21e04f17da2ef121420f36a441c2187e168fd80"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "create"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "apirequest"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"

Update the scratch/123 bookmark

  $ echo new_commit > new_commit
  $ hg add -q new_commit
  $ hg ci -m 'new commit'
  $ hg push -qr . --to "scratch/123" --force
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_name
  "scratch/123"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_kind
  "scratch"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .old_bookmark_value
  "650cba20f2f6bb385ff6fe14c21e04f17da2ef121420f36a441c2187e168fd80"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  "023bd1e40aee6b505be293ba26f31796d350df57ff9d1be37ffd6ebcd95dfd9a"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "update"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "apirequest"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"

Delete the master_bookmark

  $ hg push --delete master_bookmark --config extensions.pushrebase=
  deleting remote bookmark master_bookmark

  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_name
  "master_bookmark"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_kind
  "publishing"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .old_bookmark_value
  "4a1bfca467c5d3861ae8d5788686650dc0afffbf6bc8fbe32887a59522c30cf0"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  null
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "delete"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "apirequest"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"
