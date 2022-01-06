# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ ENABLE_PRESERVE_BUNDLE2=1 BLOB_TYPE="blob_files" quiet default_setup

Pushrebase commit

  $ hg up -q "min(all())"
  $ echo "foo" > foo
  $ hg commit -Aqm "add foo"
  $ quiet hgmn push -r . --to master_bookmark

Check bookmark history

  $ mononoke_admin bookmarks log -c bonsai master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * (master_bookmark) 2a82f3ca034e35c9d8a658c3d2d350d1d34399a9ef7854cda859b491e8723096 pushrebase * (glob)
  * (master_bookmark) c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd blobimport * (glob)

Replay the push. It will fail since the bookmark is in the wrong position

  $ unbundle_replay log-entry 2
  * Loading repository: repo (id = 0) (glob)
  *Reloading redacted config from configerator* (glob)
  * Fetching bundle from log entry: 2 (glob)
  * Fetching raw bundle: * (glob)
  * Unbundle starting: master_bookmark: Some(Bonsai(ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)))) -> Bonsai(ChangesetId(Blake2(2a82f3ca034e35c9d8a658c3d2d350d1d34399a9ef7854cda859b491e8723096))) (glob)
  * Execution error: Expected cs_id for BookmarkName { bookmark: "master_bookmark" } at Some(ChangesetId(Blake2(*))), found Some(ChangesetId(Blake2(*))) (glob)
  Error: Execution failed
  [1]

Put the bookmark back to before the push

  $ quiet mononoke_admin bookmarks set master_bookmark c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd

Replay the push. It will succeed now

  $ quiet unbundle_replay log-entry 2

Check history again. We're back to where we were:

  $ mononoke_admin bookmarks log -c bonsai master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * (master_bookmark) 2a82f3ca034e35c9d8a658c3d2d350d1d34399a9ef7854cda859b491e8723096 pushrebase * (glob)
  * (master_bookmark) c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd manualmove * (glob)
  * (master_bookmark) 2a82f3ca034e35c9d8a658c3d2d350d1d34399a9ef7854cda859b491e8723096 pushrebase * (glob)
  * (master_bookmark) c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd blobimport * (glob)

Check that we did derive fielnodes

  $ mononoke_admin derived-data exists filenodes master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(2a82f3ca034e35c9d8a658c3d2d350d1d34399a9ef7854cda859b491e8723096)) (glob)
  Derived: 2a82f3ca034e35c9d8a658c3d2d350d1d34399a9ef7854cda859b491e8723096
