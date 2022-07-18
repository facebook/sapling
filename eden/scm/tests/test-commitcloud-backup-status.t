#chg-compatible
  $ setconfig experimental.allowfilepeer=True

#require symlink

  $ enable amend smartlog
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ enable remotenames

  $ hgfakedate() {
  >   fakedate="$1"
  >   shift
  >   hg --config extensions.fakedate="$TESTDIR/fakedate.py" --config fakedate.date="$fakedate" "$@"
  > }

Setup server
  $ newserver repo
  $ cd ..

To avoid test flakiness with times, do all operations relative to 2016-01-07T12:00:00Z
  $ now=1452168000
  $ setconfig extensions.fakedate="$TESTDIR/fakedate.py" fakedate.date="$now 0"

Setup client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ touch file1
  $ hg add file1
  $ commit_time=$(($now - 15 * 60))
  $ hg commit -d "$commit_time 0" -m "Public changeset"
  $ touch file2
  $ hg add file2
  $ commit_time=$(($now - 15 * 60))
  $ hg commit -d "$commit_time 0" -m "Public changeset 2"
  $ hg push --to master --create --force
  pushing rev c46481f83c9b to destination ssh://user@dummy/repo bookmark master
  searching for changes
  exporting bookmark master
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  $ hg up 00e8a1efc6e28bc6f64a6e5f365f5ad0a2cebb11
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a > file1
  $ changeset_time=$(($now - 13 * 60))
  $ hg commit -d "$commit_time 0" -m "Backed up changeset"
  $ echo a1 > file1
  $ changeset_time=$(($now - 12 * 60))
  $ hg commit -d "$commit_time 0" -m "Backed up changeset 2"
  $ hg cloud backup
  backing up stack rooted at * (glob)
  commitcloud: backed up 2 commits
  remote: pushing 2 commits:
  remote:     *  Backed up changeset (glob)
  remote:     *  Backed up changeset 2 (glob)

Check hiding the backup head doesn't affect backed-up changesets
  $ hg up -q 6a37606e3792afff17078859861bbbbdb1227bc2
  $ hg log -T '{desc}\n' -r 'backedup()'
  Backed up changeset
  Backed up changeset 2
  $ hg log -T '{desc}\n' -r 'notbackedup()'
  $ hg hide 'max(desc(Backed))'
  hiding commit * (glob)
  1 changeset hidden
  $ hg log -T '{desc}\n' -r 'backedup()' --traceback
  Backed up changeset
  $ hg log -T '{desc}\n' -r 'notbackedup()'
  $ hg unhide 'max(desc(Backed))'
  $ hg up -q 'max(desc(Backed))'

Revset does not crash if paths.default is unset
  $ hg log -T '{desc}\n' -r 'notbackedup()' --config paths.default=
  $ hg log -T '{desc}\n' -r 'backedup()' --config paths.default=

Create some changesets that aren't backed up
  $ echo b > file1
  $ commit_time=$(($now - 11 * 60))
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset"
  $ echo c > file1
  $ commit_time=$(($now - 9 * 60))
  $ hg commit -d "$commit_time 0" -m "Backup pending changeset"

Check backup status of commits
  $ hg log -T '{desc}\n' -r 'backedup()'
  Backed up changeset
  Backed up changeset 2
  $ hg log -T '{desc}\n' -r 'draft() - backedup()'
  Not backed up changeset
  Backup pending changeset
  $ hg log -T '{desc}\n' -r 'notbackedup()'
  Not backed up changeset
  Backup pending changeset

Check smartlog output
  $ hg smartlog -T '{desc}\n' --config infinitepushbackup.autobackup=no
  o  Public changeset 2
  │
  │ @  Backup pending changeset
  │ │
  │ o  Not backed up changeset
  │ │
  │ o  Backed up changeset 2
  │ │
  │ o  Backed up changeset
  ├─╯
  o  Public changeset
  
  note: background backup is currently disabled so your commits are not being backed up.
  note: changeset * is not backed up. (glob)
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

Check smartlog summary can be suppressed
  $ hg smartlog -T '{desc}\n' --config infinitepushbackup.enablestatus=no
  o  Public changeset 2
  │
  │ @  Backup pending changeset
  │ │
  │ o  Not backed up changeset
  │ │
  │ o  Backed up changeset 2
  │ │
  │ o  Backed up changeset
  ├─╯
  o  Public changeset
  
Check smartlog summary with multiple unbacked up changesets
  $ hg up 6a37606e3792afff17078859861bbbbdb1227bc2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b2 > file1
  $ commit_time=$(($now - 11 * 60))
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset 2"
  $ hg smartlog -T '{desc}\n' --config infinitepushbackup.autobackup=yes
  o  Public changeset 2
  │
  │ o  Backup pending changeset
  │ │
  │ o  Not backed up changeset
  │ │
  │ o  Backed up changeset 2
  │ │
  │ │ @  Not backed up changeset 2
  │ ├─╯
  │ o  Backed up changeset
  ├─╯
  o  Public changeset
  
  note: 2 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

Check backup status with an unbacked up changeset that is disjoint from existing backups
  $ hg up 'max(desc(Public))'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > file2
  $ commit_time=$(($now - 11 * 60))
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset 3"
  $ hg log -T '{desc}\n' -r 'notbackedup()'
  Not backed up changeset
  Backup pending changeset
  Not backed up changeset 2
  Not backed up changeset 3

Test template keyword for when a backup is in progress
  $ hg log -T '{if(backingup,"Yes","No")}\n' -r .
  No
  $ EDENSCM_TEST_PRETEND_LOCKED=infinitepushbackup.lock hg log -T '{if(backingup,"Yes","No")}\n' -r .
  Yes

Test for infinitepushbackup disable
  $ setconfig infinitepushbackup.autobackup=true
  $ hg cloud enable
  background backup is already enabled
  $ hg cloud disable
  commitcloud: background backup is now disabled until * (glob)
  $ hg cloud disable
  note: background backup was already disabled
  commitcloud: background backup is now disabled until * (glob)
  $ hg cloud disable --hours ggg
  note: background backup was already disabled
  abort: error: argument 'hours': invalid int value: 'ggg'
  
  [255]

Test sl when infinitepushbackup is disabled but disabling has been expired / not expired
  $ hg sl -T '{desc}\n'
  @  Not backed up changeset 3
  │
  o  Public changeset 2
  │
  │ o  Backup pending changeset
  │ │
  │ o  Not backed up changeset
  │ │
  │ o  Backed up changeset 2
  │ │
  │ │ o  Not backed up changeset 2
  │ ├─╯
  │ o  Backed up changeset
  ├─╯
  o  Public changeset
  
  note: background backup is currently disabled until * (glob)
  so your commits are not being backed up.
  (run 'hg cloud enable' to turn automatic backups back on)
  note: 3 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

Advance time so that the disable has expired
  $ hg sl --config fakedate.date="1452175000 0" -T '{desc}\n' --config infinitepushbackup.enablestatus=no
  @  Not backed up changeset 3
  │
  o  Public changeset 2
  │
  │ o  Backup pending changeset
  │ │
  │ o  Not backed up changeset
  │ │
  │ o  Backed up changeset 2
  │ │
  │ │ o  Not backed up changeset 2
  │ ├─╯
  │ o  Backed up changeset
  ├─╯
  o  Public changeset
  

Hide or obsolete some of the non-backed-up commits.  The hidden commits that
have not been backed up should no longer show up as "not backed up", even if
'--hidden' is passed.  The hidden commits that have been backed up may still
show as backed up if '--hidden' is passed.

  $ echo c > file2
  $ commit_time=$(($now - 11 * 60))
  $ hg amend -d "$commit_time 0" -m "Not backed up changeset 3 (amended)"
  $ hg hide -q 9d434400bf7f325460bd0b304582414f2848ae03
  $ hg log -T '{desc}\n' -r 'backedup()'
  Backed up changeset
  $ hg log -T '{desc}\n' -r 'backedup()' --hidden
  Backed up changeset
  Backed up changeset 2
  $ hg log -T '{desc}\n' -r 'notbackedup()'
  Not backed up changeset 2
  Not backed up changeset 3 (amended)
  $ hg log -T '{desc}\n' -r 'notbackedup()' --hidden
  Not backed up changeset
  Backup pending changeset
  Not backed up changeset 2
  Not backed up changeset 3
  Not backed up changeset 3 (amended)
  $ hg sl -T '{desc}\n'
  @  Not backed up changeset 3 (amended)
  │
  o  Public changeset 2
  │
  │ o  Not backed up changeset 2
  │ │
  │ o  Backed up changeset
  ├─╯
  o  Public changeset
  
  note: background backup is currently disabled until Thu Jan 07 13:00:00 2016 +0000
  so your commits are not being backed up.
  (run 'hg cloud enable' to turn automatic backups back on)
  note: 2 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

(3, 4, 5 do not have successors. They show up as 'o' not 'x' with --hidden)
  $ hg sl -T '{desc}\n' --hidden
  @  Not backed up changeset 3 (amended)
  │
  │ x  Not backed up changeset 3
  ├─╯
  o  Public changeset 2
  │
  │ o  Backup pending changeset
  │ │
  │ o  Not backed up changeset
  │ │
  │ o  Backed up changeset 2
  │ │
  │ │ o  Not backed up changeset 2
  │ ├─╯
  │ o  Backed up changeset
  ├─╯
  o  Public changeset
  
  note: background backup is currently disabled until Thu Jan 07 13:00:00 2016 +0000
  so your commits are not being backed up.
  (run 'hg cloud enable' to turn automatic backups back on)
  note: 4 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)
