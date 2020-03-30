# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ASSIGN_GLOBALREVS=1 ENABLE_PRESERVE_BUNDLE2=1 BLOB_TYPE="blob_files" quiet default_setup

Set up script to output the raw bundle. This doesn't look at its arguments at all

  $ BUNDLE_ROOT="$(realpath "${TESTTMP}/bundles")"
  $ BUNDLE_HELPER="$(realpath "${TESTTMP}/bundle_helper.sh")"
  $ cat > "$BUNDLE_HELPER" <<EOF
  > #!/bin/bash
  > cat "$BUNDLE_ROOT/\$1"
  > EOF
  $ chmod +x "$BUNDLE_HELPER"

Pushrebase commit

  $ hg up -q 0
  $ echo "foo" > foo
  $ hg commit -Aqm "add foo"
  $ hg log -r 0::. -T '{node}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  4afe8a7fa62cf8320c8c11191d4dfdaaed9fb28b
  $ quiet hgmn push -r . --to master_bookmark

  $ hg up -q 0
  $ echo "bar" > bar
  $ hg commit -Aqm "add bar"
  $ hg log -r 0::. -T '{node}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  87e97d0197df950005fa12535b98eaed237c2d2f
  $ quiet hgmn push -r . --to master_bookmark

  $ hg log -r ::master_bookmark -T '{node}\n'
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  112478962961147124edd43549aedd1a335e44bf
  26805aba1e600a82e93661149f2313866a221a7b
  cbab85d064b0fbdd3e9caa125f8eeac0fb5acf6a
  7a8f33ce453248a6f5cc4747002e931c77234fbd

Check bookmark history

  $ mononoke_admin bookmarks log -c hg master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  (master_bookmark) 7a8f33ce453248a6f5cc4747002e931c77234fbd pushrebase * (glob)
  (master_bookmark) cbab85d064b0fbdd3e9caa125f8eeac0fb5acf6a pushrebase * (glob)
  (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

Export the bundles so we can replay it as it if were coming from hg, through the $BUNDLE_HELPER

  $ mkdir "$BUNDLE_ROOT"
  $ quiet mononoke_admin hg-sync-bundle fetch-bundle --id 2 --output-file "$BUNDLE_ROOT/bundle1"
  $ quiet mononoke_admin hg-sync-bundle fetch-bundle --id 3 --output-file "$BUNDLE_ROOT/bundle2"

Blow everything away: we're going to re-do the push from scratch, in a new repo.

  $ kill -9 "$MONONOKE_PID"
  $ rm -rf "$TESTTMP/mononoke-config" "$TESTTMP/monsql" "$TESTTMP/blobstore"
  $ ASSIGN_GLOBALREVS=1 BLOB_TYPE="blob_files" quiet default_setup

Replay the push. This will not do anything because the entries do not exist (we need run this once to create the schema).

  $ unbundle_replay hg-bookmark "$BUNDLE_HELPER" master_bookmark
  * Loading repository: repo (id = 0) (glob)
  * Loading hg bookmark updates for bookmark master_bookmark, starting at 26805aba1e600a82e93661149f2313866a221a7b (glob)
  * No further hg bookmark updates for bookmark master_bookmark at 26805aba1e600a82e93661149f2313866a221a7b (glob)

Insert the entry. Note that in tests, the commit timestamp will always be zero.

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" << EOS
  > INSERT INTO pushrebaserecording(repo_id, onto, ontorev, bundlehandle, timestamps, ordered_added_revs) VALUES
  > (
  >   0,
  >   'master_bookmark',
  >   '26805aba1e600a82e93661149f2313866a221a7b',
  >   'bundle1',
  >   '{"4afe8a7fa62cf8320c8c11191d4dfdaaed9fb28b": [0.0, 0]}',
  >   '["cbab85d064b0fbdd3e9caa125f8eeac0fb5acf6a"]'
  > ),
  > (
  >   0,
  >   'master_bookmark',
  >   'cbab85d064b0fbdd3e9caa125f8eeac0fb5acf6a',
  >   'bundle2',
  >   '{"87e97d0197df950005fa12535b98eaed237c2d2f": [0.0, 0]}',
  >   '["7a8f33ce453248a6f5cc4747002e931c77234fbd"]'
  > );
  > EOS

Replay the push. It will succeed now

  $ quiet unbundle_replay hg-bookmark "$BUNDLE_HELPER" master_bookmark

Check history again. We're back to where we were:

  $ mononoke_admin bookmarks log -c hg master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  (master_bookmark) 7a8f33ce453248a6f5cc4747002e931c77234fbd pushrebase * (glob)
  (master_bookmark) cbab85d064b0fbdd3e9caa125f8eeac0fb5acf6a pushrebase * (glob)
  (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)
