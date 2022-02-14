# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Set up local hgrc and Mononoke config.
  $ quiet default_setup_blobimport
  $ configure modern

Build up segmented changelog
  $ quiet segmented_changelog_tailer_reseed --repo-name=repo --head=master_bookmark
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select version, vertex, hex(cs_id) from segmented_changelog_idmap"
  1|0|9FEB8DDD3E8EDDCFA3A4913B57DF7842BEDF84B8EA3B7B3FCB14C6424AA81FEC
  1|1|459F16AE564C501CB408C1E5B60FC98A1E8B8E97B9409C7520658BFA1577FB66
  1|2|C3384961B16276F2DB77DF9D7C874BBE981CF0525BD6F84A502F919044F2DABD

Enable Segmented Changelog
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > enabled=true
  > master_bookmark="master_bookmark"
  > CONFIG

  $ mononoke
  $ wait_for_mononoke

Lazy clone the repo from mononoke 
  $ cd "$TESTTMP"
  $ setconfig remotenames.selectivepull=True remotenames.selectivepulldefault=master_bookmark
  $ setconfig pull.httpcommitgraph=1 pull.httphashprefix=1
  $ hgedenapi clone "mononoke://$(mononoke_address)/repo" repo2
  fetching lazy changelog
  populating main commit graph
  tip commit: 26805aba1e600a82e93661149f2313866a221a7b
  fetching selected remote bookmarks
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd "repo2"
  $ enable pushrebase remotenames

Make a new commit
  $ hgedenapi up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "D" > "D"
  $ hgedenapi ci -m "D" -A D
  $ hgedenapi push -r . --to master_bookmark
  pushing rev c2f72b3cb5e9 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  $ hgedenapi log -G -T '{node} {desc} {remotenames} {bookmarks}\n' -r "all()"
  @  c2f72b3cb5e9ea5ce6b764fc5b4f7c7b23208217 D remote/master_bookmark
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C
  │
  o  112478962961147124edd43549aedd1a335e44bf B
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  

Move bookmark backwards
  $ REPOID=0 mononoke_admin bookmarks set master_bookmark "$C"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Reloading redacted config from configerator (glob)
  * changeset resolved as: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)) (glob)
  * Current position of BookmarkName { bookmark: "master_bookmark" } is Some(ChangesetId(Blake2(830c5dcdb5335a4ecdda15afc927edd525c26d0a3d7f14c93ef5ecc48d0db532))) (glob)

Restart mononoke, this way we drop in-memory segmented changelog representations.
  $ killandwait $MONONOKE_PID
  $ start_and_wait_for_mononoke_server

Check that bookmark moved correctly
  $ hgedenapi debugapi -e bookmarks -i '["master_bookmark", "master"]'
  {"master": None,
   "master_bookmark": "26805aba1e600a82e93661149f2313866a221a7b"}

  $ merge_tunables <<EOF
  > {
  >   "ints": {
  >     "segmented_changelog_client_max_commits_to_traverse": 100
  >   }
  > }
  > EOF

Pull should succeed and local bookmark should be moved back.
  $ LOG=pull::fastpath=debug hgedenapi pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  DEBUG pull::fastpath: master: c2f72b3cb5e9ea5ce6b764fc5b4f7c7b23208217 => 26805aba1e600a82e93661149f2313866a221a7b
  imported commit graph for 0 commits (0 segments)

Check that segmented changelog IdMap in DB didn't change. 
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select version, vertex, hex(cs_id) from segmented_changelog_idmap"
  1|0|9FEB8DDD3E8EDDCFA3A4913B57DF7842BEDF84B8EA3B7B3FCB14C6424AA81FEC
  1|1|459F16AE564C501CB408C1E5B60FC98A1E8B8E97B9409C7520658BFA1577FB66
  1|2|C3384961B16276F2DB77DF9D7C874BBE981CF0525BD6F84A502F919044F2DABD
  $ hgedenapi log -G -T '{node} {desc} {remotenames} {bookmarks}\n' -r "all()"
  @  c2f72b3cb5e9ea5ce6b764fc5b4f7c7b23208217 D
  │
  o  26805aba1e600a82e93661149f2313866a221a7b C remote/master_bookmark
  │
  o  112478962961147124edd43549aedd1a335e44bf B
  │
  o  426bada5c67598ca65036d57d9e4b64b0c1ce7a0 A
  
