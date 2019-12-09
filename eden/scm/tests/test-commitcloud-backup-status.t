#chg-compatible

#require symlink

  $ enable amend smartlog
  $ setconfig infinitepushbackup.enablestatus=true
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

  $ hgfakedate() {
  >   fakedate="$1"
  >   shift
  >   hg --config extensions.fakedate="$TESTDIR/fakedate.py" --config fakedate.date="$fakedate" "$@"
  > }

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

To avoid test flakiness with times, do all operations relative to 2016-01-07T12:00:00Z
  $ now=1452168000
  $ setconfig extensions.fakedate="$TESTDIR/fakedate.py" fakedate.date="$now 0"

Setup client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ touch file1
  $ hg add file1
  $ commit_time=`expr $now - 15 \* 60`
  $ hg commit -d "$commit_time 0" -m "Public changeset"
  $ touch file2
  $ hg add file2
  $ commit_time=`expr $now - 15 \* 60`
  $ hg commit -d "$commit_time 0" -m "Public changeset 2"
  $ hg push
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 2 files
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a > file1
  $ changeset_time=`expr $now - 13 \* 60`
  $ hg commit -d "$commit_time 0" -m "Backed up changeset"
  $ echo a1 > file1
  $ changeset_time=`expr $now - 12 \* 60`
  $ hg commit -d "$commit_time 0" -m "Backed up changeset 2"
  $ hg cloud backup
  backing up stack rooted at * (glob)
  remote: pushing 2 commits:
  remote:     *  Backed up changeset (glob)
  remote:     *  Backed up changeset 2 (glob)
  commitcloud: backed up 2 commits

Check hiding the backup head doesn't affect backed-up changesets
  $ hg up -q 2
  $ hg log -T '{rev} {desc}\n' -r 'backedup()'
  2 Backed up changeset
  3 Backed up changeset 2
  $ hg log -T '{rev} {desc}\n' -r 'notbackedup()'
  $ hg hide 3
  hiding commit * (glob)
  1 changeset hidden
  $ hg log -T '{rev} {desc}\n' -r 'backedup()' --traceback
  2 Backed up changeset
  $ hg log -T '{rev} {desc}\n' -r 'notbackedup()'
  $ hg unhide 3
  $ hg up -q 3

Create some changesets that aren't backed up
  $ echo b > file1
  $ commit_time=`expr $now - 11 \* 60`
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset"
  $ echo c > file1
  $ commit_time=`expr $now - 9 \* 60`
  $ hg commit -d "$commit_time 0" -m "Backup pending changeset"

Check backup status of commits
  $ hg log -T '{rev} {desc}\n' -r 'backedup()'
  2 Backed up changeset
  3 Backed up changeset 2
  $ hg log -T '{rev} {desc}\n' -r 'draft() - backedup()'
  4 Not backed up changeset
  5 Backup pending changeset
  $ hg log -T '{rev} {desc}\n' -r 'notbackedup()'
  4 Not backed up changeset
  5 Backup pending changeset

Check smartlog output
  $ hg smartlog -T '{rev}: {desc}\n' --config infinitepushbackup.autobackup=no
  o  1: Public changeset 2
  |
  | @  5: Backup pending changeset
  | |
  | o  4: Not backed up changeset
  | |
  | o  3: Backed up changeset 2
  | |
  | o  2: Backed up changeset
  |/
  o  0: Public changeset
  
  note: background backup is currently disabled so your commits are not being backed up.
  note: changeset * is not backed up. (glob)
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

Check smartlog summary can be suppressed
  $ hg smartlog -T '{rev}: {desc}\n' --config infinitepushbackup.enablestatus=no
  o  1: Public changeset 2
  |
  | @  5: Backup pending changeset
  | |
  | o  4: Not backed up changeset
  | |
  | o  3: Backed up changeset 2
  | |
  | o  2: Backed up changeset
  |/
  o  0: Public changeset
  
Check smartlog summary with multiple unbacked up changesets
  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b2 > file1
  $ commit_time=`expr $now - 11 \* 60`
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset 2"
  $ hg smartlog -T '{rev}: {desc}\n' --config infinitepushbackup.autobackup=yes
  o  1: Public changeset 2
  |
  | @  6: Not backed up changeset 2
  | |
  | | o  5: Backup pending changeset
  | | |
  | | o  4: Not backed up changeset
  | | |
  | | o  3: Backed up changeset 2
  | |/
  | o  2: Backed up changeset
  |/
  o  0: Public changeset
  
  note: 2 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

Check backup status with an unbacked up changeset that is disjoint from existing backups
  $ hg up 1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > file2
  $ commit_time=`expr $now - 11 \* 60`
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset 3"
  $ hg log -T '{rev} {desc}\n' -r 'notbackedup()'
  4 Not backed up changeset
  5 Backup pending changeset
  6 Not backed up changeset 2
  7 Not backed up changeset 3

Test template keyword for when a backup is in progress
  $ hg log -T '{if(backingup,"Yes","No")}\n' -r .
  No
  $ rm -f .hg/infinitepushbackup.lock
  $ ln -s fakelock .hg/infinitepushbackup.lock
  $ hg log -T '{if(backingup,"Yes","No")}\n' -r .
  Yes
  $ rm -f .hg/infinitepushbackup.lock

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
  $ hg sl -T '{rev} {desc}\n'
  @  7 Not backed up changeset 3
  |
  o  1 Public changeset 2
  |
  | o  6 Not backed up changeset 2
  | |
  | | o  5 Backup pending changeset
  | | |
  | | o  4 Not backed up changeset
  | | |
  | | o  3 Backed up changeset 2
  | |/
  | o  2 Backed up changeset
  |/
  o  0 Public changeset
  
  note: background backup is currently disabled until * (glob)
  so your commits are not being backed up.
  (run 'hg cloud enable' to turn automatic backups back on)
  note: 3 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

Advance time so that the disable has expired
  $ hg sl --config fakedate.date="1452175000 0" -T '{rev} {desc}\n'
  @  7 Not backed up changeset 3
  |
  o  1 Public changeset 2
  |
  | o  6 Not backed up changeset 2
  | |
  | | o  5 Backup pending changeset
  | | |
  | | o  4 Not backed up changeset
  | | |
  | | o  3 Backed up changeset 2
  | |/
  | o  2 Backed up changeset
  |/
  o  0 Public changeset
  
  note: 4 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)

Hide or obsolete some of the non-backed-up commits.  The hidden commits that
have not been backed up should no longer show up as "not backed up", even if
'--hidden' is passed.  The hidden commits that have been backed up may still
show as backed up if '--hidden' is passed.

  $ echo c > file2
  $ commit_time=`expr $now - 11 \* 60`
  $ hg amend -d "$commit_time 0" -m "Not backed up changeset 3 (amended)"
  $ hg hide -q 3
  $ hg log -T '{rev} {desc}\n' -r 'backedup()'
  2 Backed up changeset
  $ hg log -T '{rev} {desc}\n' -r 'backedup()' --hidden
  2 Backed up changeset
  3 Backed up changeset 2
  $ hg log -T '{rev} {desc}\n' -r 'notbackedup()'
  6 Not backed up changeset 2
  8 Not backed up changeset 3 (amended)
  $ hg log -T '{rev} {desc}\n' -r 'notbackedup()' --hidden
  6 Not backed up changeset 2
  8 Not backed up changeset 3 (amended)
  $ hg sl -T '{rev} {desc}\n'
  @  8 Not backed up changeset 3 (amended)
  |
  o  1 Public changeset 2
  |
  | o  6 Not backed up changeset 2
  | |
  | o  2 Backed up changeset
  |/
  o  0 Public changeset
  
  note: background backup is currently disabled until Thu Jan 07 13:00:00 2016 +0000
  so your commits are not being backed up.
  (run 'hg cloud enable' to turn automatic backups back on)
  note: 2 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)
  $ hg sl -T '{rev} {desc}\n' --hidden
  @  8 Not backed up changeset 3 (amended)
  |
  | x  7 Not backed up changeset 3
  |/
  o  1 Public changeset 2
  |
  | o  6 Not backed up changeset 2
  | |
  | | x  5 Backup pending changeset
  | | |
  | | x  4 Not backed up changeset
  | | |
  | | x  3 Backed up changeset 2
  | |/
  | o  2 Backed up changeset
  |/
  o  0 Public changeset
  
  note: background backup is currently disabled until Thu Jan 07 13:00:00 2016 +0000
  so your commits are not being backed up.
  (run 'hg cloud enable' to turn automatic backups back on)
  note: 2 changesets are not backed up.
  (run 'hg cloud backup' to perform a backup)
  (if this fails, please report to the Source Control Team)
