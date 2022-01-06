# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration

  $ export SCUBA_CENSORED_LOGGING_PATH="$TESTTMP/censored_scuba.json"
  $ REPOTYPE="blob_files"
  $ setup_common_config $REPOTYPE
  $ enable remotenames

  $ cd $TESTTMP

setup hg server repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg ci -A -q -m 'add a'

  $ hg log -T '{short(node)}\n'
  ac82d8b1f7c4

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-pull and repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push2 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push3 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull-fetchpacksv1 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull-fetchpacksv2 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull-edenapi --noupdate
  $ cat >> repo-pull-fetchpacksv1/.hg/hgrc << EOF
  > [remotefilelog]
  > fetchpacks=True
  > getpackversion=1
  > EOF
  $ cat >> repo-pull-fetchpacksv2/.hg/hgrc << EOF
  > [remotefilelog]
  > fetchpacks=True
  > getpackversion=2
  > EOF

blobimport
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > rebase =
  > EOF

  $ cd ../repo-push2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ cd ../repo-push3
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > rebase =
  > EOF

  $ cd ../repo-push

  $ hgmn up -q 0
Push files
  $ echo b > b
  $ hg ci -A -q -m "add b"

  $ hgmn push -q -r .  --to master_bookmark

  $ echo c > c
  $ hg ci -A -q -m "add censored c"

  $ hgmn push -q -r .  --to master_bookmark

  $ tglogpnr
  @  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ cd "$TESTTMP/repo-push2"
  $ hgmn pull -q

  $ hgmn up -q 064d994d0240
  $ tglogpnr
  @  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ cd "$TESTTMP/repo-pull"

  $ hgmn pull -q

  $ tglogpnr
  o  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ cd "$TESTTMP/repo-push3"

  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ tglogpnr
  o  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ hgmn up master_bookmark
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Update redacted blob
  $ cd "$TESTTMP/repo-push"
  $ echo "testcupdate" > c
  $ hg ci -q -m "uncensore c"
  $ hgmn push -q -r .  --to master_bookmark

  $ echo "log-only" > log_only
  $ hg add log_only
  $ hg ci -q -m "log-only"
  $ hgmn push -q -r .  --to master_bookmark

  $ tglogpnr
  @  73f850a22540 public 'log-only'  default/master_bookmark
  │
  o  bbb84cdc8ec0 public 'uncensore c'
  │
  o  064d994d0240 public 'add censored c'
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ hg log -T '{node}\n'
  73f850a225400422723d433ab3ea194c09c2c8c5
  bbb84cdc8ec039fe71d78a3adb6f5cf244fafb6a
  064d994d0240f9738dba1ef7479f0a4ce8486b05
  14961831bd3af3a6331fef7e63367d61cb6c9f6b
  ac82d8b1f7c418c61a493ed229ffaa981bda8e90

Censore the redacted blob (file 'c' in commit '064d994d0240f9738dba1ef7479f0a4ce8486b05')
  $ MONONOKE_EXEC_STAGE=admin mononoke_admin redaction create-key-list 064d994d0240f9738dba1ef7479f0a4ce8486b05 c --force | head -n 1 | sed 's/Redaction saved as: //g' > rs_1
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: * (glob)
  $ MONONOKE_EXEC_STAGE=admin mononoke_admin redaction create-key-list 73f850a225400422723d433ab3ea194c09c2c8c5 log_only --force | head -n 1 | sed 's/Redaction saved as: //g' > rs_2
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * changeset resolved as: ChangesetId(Blake2(aac5f17ddfcadf26a410f701b860b2a7c7d5c082cec420b816296014660f7fca)) (glob)
  $ cat > "$REDACTION_CONF/redaction_sets" <<EOF
  > {
  >  "all_redactions": [
  >    {"reason": "T1", "id": "$(cat rs_1)", "enforce": true},
  >    {"reason": "T2", "id": "$(cat rs_2)", "enforce": false}
  >  ]
  > }
  > EOF
  $ rm rs_1 rs_2

# We could not restart mononoke here, but then we'd have to wait 60s for it to
# update the redaction config automatically
Restart mononoke
  $ killandwait $MONONOKE_PID
  $ rm -rf "$TESTTMP/mononoke-config"
  $ setup_common_config blob_files
  $ mononoke
  $ wait_for_mononoke

  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  o  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ hgmn up master_bookmark
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ tglogpnr
  @  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

The content of the redacted file is replaced by a string
  $ cat c
  This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.

Diff and Status should not see any change made to file c, even if it contains the magic string
  $ hgmn status
  $ hgmn diff

Try push a new version of a redacted blob
  $ cd "$TESTTMP/repo-push2"
  $ touch "test12" > c
  $ hg ci -q -m "update c"

  $ tglogpnr
  @  bb65510879c8 draft 'update c'
  │
  o  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

As of the time of writing, updating redacted files throws an error - artifact of the existing implementation.
  $ hgmn push -r . --to master_bookmark
  pushing rev bb65510879c8 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     pushrebase failed Conflicts([PushrebaseConflict { left: MPath("c"), right: MPath("c") }])
  remote: 
  remote:   Root cause:
  remote:     pushrebase failed Conflicts([PushrebaseConflict { left: MPath("c"), right: MPath("c") }])
  remote: 
  remote:   Debug context:
  remote:     "pushrebase failed Conflicts([PushrebaseConflict { left: MPath(\"c\"), right: MPath(\"c\") }])"
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ tglogpnr
  @  bb65510879c8 draft 'update c'
  │
  o  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ cd "$TESTTMP/repo-pull"
  $ tglogpnr
  @  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ hgmn pull -q

  $ tglogpnr
  o  73f850a22540 public 'log-only'  default/master_bookmark
  │
  o  bbb84cdc8ec0 public 'uncensore c'
  │
  @  064d994d0240 public 'add censored c'
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ hgmn up 064d994d0240
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Expect success (no blob in this commit is redacted)
  $ hgmn up bbb84cdc8ec0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ tglogpnr
  o  73f850a22540 public 'log-only'  default/master_bookmark
  │
  @  bbb84cdc8ec0 public 'uncensore c'
  │
  o  064d994d0240 public 'add censored c'
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

Test rebasing local commit on top of master_bookmark, when base commit contains censored blob
  $ cd "$TESTTMP/repo-push3"
  $ tglogpnr
  @  064d994d0240 public 'add censored c'  default/master_bookmark
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ echo "aa" > a
  $ hg ci -q -m "update a"

  $ hgmn pull -q
  $ tglogpnr
  o  73f850a22540 public 'log-only'  default/master_bookmark
  │
  o  bbb84cdc8ec0 public 'uncensore c'
  │
  │ @  c6e4e7cae299 draft 'update a'
  ├─╯
  o  064d994d0240 public 'add censored c'
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

Should be successful
  $ hgmn rebase -s . -d bbb84cdc8ec0
  rebasing c6e4e7cae299 "update a"

  $ tglogpnr
  @  d967612e0cc1 draft 'update a'
  │
  │ o  73f850a22540 public 'log-only'  default/master_bookmark
  ├─╯
  o  bbb84cdc8ec0 public 'uncensore c'
  │
  o  064d994d0240 public 'add censored c'
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

  $ hgmn up -q 064d994d0240
  $ echo "bb" > b

  $ tglogpnr
  o  d967612e0cc1 draft 'update a'
  │
  │ o  73f850a22540 public 'log-only'  default/master_bookmark
  ├─╯
  o  bbb84cdc8ec0 public 'uncensore c'
  │
  @  064d994d0240 public 'add censored c'
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

Updating from a commit that contains a redacted file to another commit should succeed
  $ hgmn up -q bbb84cdc8ec0

  $ tglogpnr
  o  d967612e0cc1 draft 'update a'
  │
  │ o  73f850a22540 public 'log-only'  default/master_bookmark
  ├─╯
  @  bbb84cdc8ec0 public 'uncensore c'
  │
  o  064d994d0240 public 'add censored c'
  │
  o  14961831bd3a public 'add b'
  │
  o  ac82d8b1f7c4 public 'add a'
  

File should contain the uncommited change: bb
  $ cat b
  bb

Updating to a commit with censored files works in getpackv1 repo
  $ cd $TESTTMP/repo-pull-fetchpacksv1
  $ hgmn pull -q
  $ hgmn up -q 064d994d0240
  $ cat c
  This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.

Updating to a commit with censored files works in getpackv2 repo
  $ cd $TESTTMP/repo-pull-fetchpacksv2
  $ hgmn pull -q
  $ hgmn up -q 064d994d0240
  $ cat c
  This version of the file is redacted and you are not allowed to access it. Update or rebase to a newer commit.

Update to log_only commit
  $ hgmn up -q 73f850a225400422723d433ab3ea194c09c2c8c5
  $ cat log_only
  log-only

Check logging
  $ wait_for_json_record_count "$TESTTMP/censored_scuba.json" 4
  $ format_single_scuba_sample < "$TESTTMP/censored_scuba.json"
  {
    "int": {
      "time": * (glob)
    },
    "normal": {
      "key": "content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2",
      "operation": "GET",
      "session_uuid": "*", (glob)
      "unix_username": "*" (glob)
    }
  }
  {
    "int": {
      "time": * (glob)
    },
    "normal": {
      "key": "content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2",
      "operation": "GET",
      "session_uuid": "*", (glob)
      "unix_username": "*" (glob)
    }
  }
  {
    "int": {
      "time": * (glob)
    },
    "normal": {
      "key": "content.blake2.096c8cc4a38f793ac05fc3506ed6346deb5b857100642adbf4de6720411b10e2",
      "operation": "GET",
      "session_uuid": "*", (glob)
      "unix_username": "*" (glob)
    }
  }
  {
    "int": {
      "time": * (glob)
    },
    "normal": {
      "key": "content.blake2.8e86b59a7708c54ab97f95db4a39e27886e5d3775c24a7d0d8106f82b3d38d49",
      "operation": "GET",
      "session_uuid": "*", (glob)
      "unix_username": "*" (glob)
    }
  }
