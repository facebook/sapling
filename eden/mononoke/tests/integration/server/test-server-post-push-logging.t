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
  $ ENABLE_API_WRITES=1 INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo
  $ cd repo
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

setup master bookmarks

  $ hg bookmark master_bookmark -r 'tip'

verify content
  $ hg log
  commit:      0e7ec5675652
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

  $ cd $TESTTMP
  $ blobimport repo/.hg repo

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hg clone -q mono:repo repo2
  $ hg clone -q mono:repo repo3
  $ cd repo2
  $ hg pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found

start mononoke

  $ start_and_wait_for_mononoke_server
BEGIN Creation of new commits

create new commits in repo2 and check that they are seen as outgoing

  $ mkdir b_dir
  $ echo "new a file content" > a
  $ echo "b file content" > b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb
  $ hg push -r . --to master_bookmark --create --config extensions.pushrebase=
  pushing rev bb0985934a0f to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 2 files for upload
  edenapi: uploaded 2 files
  edenapi: queue 2 trees for upload
  edenapi: uploaded 2 trees
  edenapi: uploaded 1 changeset
  pushrebasing stack (0e7ec5675652, bb0985934a0f] (1 commit) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to bb0985934a0f

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
  "022352db2112d2f43ca2635686a6275ade50d612865551fa8d1f392b375e412e"
  "022352db2112d2f43ca2635686a6275ade50d612865551fa8d1f392b375e412e"
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .bubble_id
  null
  null
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq .parents
  [
    "30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473"
  ]
  [
    "30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473"
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
  "30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  "022352db2112d2f43ca2635686a6275ade50d612865551fa8d1f392b375e412e"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "pushrebase"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "pushrebase"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"

  $ echo forcepushrebase > forcepushrebase
  $ hg add -q forcepushrebase
  $ hg ci -m forcepushrebase
  $ hg push -r . --to forcepushrebase --create --force --config extensions.pushrebase=
  pushing rev 0c1e5152244c to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark forcepushrebase
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
  "cf79ab3ba838b597ca4973ba397b4b687f54d9eed2f0edc4f950f3b80a68f8b3"
  "cf79ab3ba838b597ca4973ba397b4b687f54d9eed2f0edc4f950f3b80a68f8b3"
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
  "cf79ab3ba838b597ca4973ba397b4b687f54d9eed2f0edc4f950f3b80a68f8b3"
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
  "f76800ae3d688512180e7a0805ff18d39f7ea81617bce1aea4e11364584b007a"
  $ rm "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY"

Use infinitepush push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > commitcloud=
  > infinitepush=
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:^scratch/.+$
  > EOF

Stop tracking master_bookmark
  $ hg up -q 0e7ec5675652
  $ echo infinitepush > infinitepush
  $ hg add -q infinitepush
  $ hg ci -m 'infinitepush'
  $ hg push -r . --to "scratch/123" --create
  pushing to mono:repo
  searching for changes
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq 'select(.is_public == false)' | jq .bookmark
  "scratch/123"
  $ cat "$TESTTMP/scribe_logs/$COMMIT_SCRIBE_CATEGORY" | jq 'select(.is_public == false)' | jq .changeset_id
  "06b8cee4d65704bcb81b988c1153daee3063d9e565f4d65e9e68475676b2438b"

  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_name
  "scratch/123"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_kind
  "scratch"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .old_bookmark_value
  null
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  "06b8cee4d65704bcb81b988c1153daee3063d9e565f4d65e9e68475676b2438b"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "create"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "push"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"

Update the scratch/123 bookmark

  $ echo new_commit > new_commit
  $ hg add -q new_commit
  $ hg ci -m 'new commit'
  $ hg push -r . --to "scratch/123"
  pushing to mono:repo
  searching for changes
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .repo_name
  "repo"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_name
  "scratch/123"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .bookmark_kind
  "scratch"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .old_bookmark_value
  "06b8cee4d65704bcb81b988c1153daee3063d9e565f4d65e9e68475676b2438b"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  "cde64fba54d56734c1ee9c2c2c2f61bc70f8407d1bab219a7c2bee524df35386"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "update"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "push"
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
  "022352db2112d2f43ca2635686a6275ade50d612865551fa8d1f392b375e412e"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .new_bookmark_value
  null
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .operation
  "delete"
  $ cat "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY" | jq .update_reason
  "apirequest"
  $ rm "$TESTTMP/scribe_logs/$BOOKMARK_SCRIBE_CATEGORY"
