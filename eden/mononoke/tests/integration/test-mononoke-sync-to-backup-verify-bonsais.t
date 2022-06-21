# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ REPOID=0 REPONAME=repo setup_common_config blob_files
  $ REPOID=1 REPONAME=backup setup_common_config blob_files
  $ export BACKUP_REPO_ID=1
  $ cd $TESTTMP

setup repo
  $ start_and_wait_for_mononoke_server
  $ cd $TESTTMP
  $ hgmn_init repo
  $ cd repo
  $ echo B > B
  $ hg add B
  $ hg ci -m 'B'
  $ hgmn push -r . --to master_bookmark --create
  pushing rev c0e1f5917744 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  exporting bookmark master_bookmark

get content_id for file B
  $ mononoke_admin bonsai-fetch master_bookmark 2> /dev/null | grep BonsaiChangesetId
  BonsaiChangesetId: d0356578495b2a286e817587034d9fbda1eb317d619496ee03a211f34d9e06da 
  $ mononoke_newadmin filestore -R repo store B
  Wrote 122e93be74ea1962717796ad5b1f4a428f431d4d4f9674846443f1e91a690b14 (2 bytes)

upload C as it wasn't imported
  $ echo C > C
  $ mononoke_newadmin filestore -R repo store C
  Wrote 2b574f3e5fdc3151a85d8982a46b82d91fa0ef0bb15224fac5a25488b69d38eb (2 bytes)
  $ cd $TESTTMP

  $ cat > bonsai_file <<EOF
  > {
  >   "parents": [
  >     "d0356578495b2a286e817587034d9fbda1eb317d619496ee03a211f34d9e06da"
  >   ],
  >   "author": "test",
  >   "author_date": "1970-01-01T00:00:00+00:00",
  >   "committer": null,
  >   "committer_date": null,
  >   "message": "C",
  >   "extra": {},
  >   "file_changes": {
  >     "C": {
  >       "Change": {
  >         "inner": {
  >           "content_id": "2b574f3e5fdc3151a85d8982a46b82d91fa0ef0bb15224fac5a25488b69d38eb",
  >           "file_type": "Regular",
  >           "size": 2
  >         },
  >         "copy_from": null
  >       }
  >     },
  >     "B": {
  >       "Change": {
  >         "inner": {
  >           "content_id": "122e93be74ea1962717796ad5b1f4a428f431d4d4f9674846443f1e91a690b14",
  >           "file_type": "Regular",
  >           "size": 2
  >         },
  >         "copy_from": null
  >       }
  >     }
  >   }
  > }
  > EOF

  $ mononoke_testtool create-bonsai -R repo bonsai_file
  Created bonsai changeset 2fd0d90fc6899dd5643e344ebad05bbd6014382de3341654a7630de99bb1f96f for Hg changeset 1ef6b45b6561464f92b16aba791974a9bb858ce2
  $ mononoke_admin bookmarks set master_bookmark 2fd0d90fc6899dd5643e344ebad05bbd6014382de3341654a7630de99bb1f96f
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: ChangesetId(Blake2(2fd0d90fc6899dd5643e344ebad05bbd6014382de3341654a7630de99bb1f96f)) (glob)
  * Current position of BookmarkName { bookmark: "master_bookmark" } is Some(ChangesetId(Blake2(d0356578495b2a286e817587034d9fbda1eb317d619496ee03a211f34d9e06da))) (glob)

Sync to backup repo
  $ mononoke_backup_sync backup sync-loop 0 2>&1 | grep 'should map' | head -n 1
  remote:     Hg changeset 1ef6b45b6561464f92b16aba791974a9bb858ce2 should map to 2fd0d90fc6899dd5643e344ebad05bbd6014382de3341654a7630de99bb1f96f, but it actually maps to 732435121e2c431e5111573abe055768657d653cda568f959c472721aca5b074 in backup

  $ mononoke_admin bonsai-fetch master_bookmark 2> /dev/null | grep BonsaiChangesetId
  BonsaiChangesetId: 2fd0d90fc6899dd5643e344ebad05bbd6014382de3341654a7630de99bb1f96f 
  $ REPOID=1 mononoke_admin bonsai-fetch master_bookmark 2> /dev/null | grep BonsaiChangesetId
  BonsaiChangesetId: d0356578495b2a286e817587034d9fbda1eb317d619496ee03a211f34d9e06da 

Run again, and make sure it fails again. This case tests that after restart hg sync job
does not accidentally skip the bundle
  $ mononoke_backup_sync backup sync-loop 0 2>&1 | grep 'should map' | head -n 1
  remote:     Hg changeset 1ef6b45b6561464f92b16aba791974a9bb858ce2 should map to 2fd0d90fc6899dd5643e344ebad05bbd6014382de3341654a7630de99bb1f96f, but it actually maps to 732435121e2c431e5111573abe055768657d653cda568f959c472721aca5b074 in backup
