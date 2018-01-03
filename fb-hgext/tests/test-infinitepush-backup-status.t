
  $ setup() {
  > cat << EOF >> .hg/hgrc
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
  > inhibit=$TESTDIR/../hgext3rd/inhibit.py
  > smartlog=$TESTDIR/../hgext3rd/smartlog.py
  > [infinitepushbackup]
  > enablestatus = True
  > [experimental]
  > evolution=createmarkers
  > EOF
  > }
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Setup client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ setup
  $ now=`date +%s`
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
  created new head
  $ echo a1 > file1
  $ changeset_time=`expr $now - 12 \* 60`
  $ hg commit -d "$commit_time 0" -m "Backed up changeset 2"
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     *  Backed up changeset (glob)
  remote:     *  Backed up changeset 2 (glob)
  finished in \d+\.(\d+)? seconds (re)

Check hiding the backup head doesn't affect backed-up changesets
  $ hg up -q 2
  $ hg log -T '{rev} {desc}\n' -r 'backedup()'
  2 Backed up changeset
  3 Backed up changeset 2
  $ hg log -T '{rev} {desc}\n' -r 'notbackedup()'
  $ hg hide 3
  1 changesets hidden
  $ hg log -T '{rev} {desc}\n' -r 'backedup()'
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
  $ hg smartlog
  o  changeset:   1:* (glob)
  |  user:        test
  |  date:        * (glob)
  |  summary:     Public changeset 2
  |
  | @  changeset:   5:* (glob)
  | |  tag:         tip
  | |  user:        test
  | |  date:        * (glob)
  | |  summary:     Backup pending changeset
  | |
  | o  changeset:   4:* (glob)
  | |  user:        test
  | |  date:        * (glob)
  | |  summary:     Not backed up changeset
  | |
  | o  changeset:   3:* (glob)
  | |  user:        test
  | |  date:        * (glob)
  | |  summary:     Backed up changeset 2
  | |
  | o  changeset:   2:* (glob)
  |/   parent:      0:* (glob)
  |    user:        test
  |    date:        * (glob)
  |    summary:     Backed up changeset
  |
  o  changeset:   0:* (glob)
     user:        test
     date:        * (glob)
     summary:     Public changeset
  
  note: changeset * is not backed up. (glob)
  Run `hg pushbackup` to perform a backup.  If this fails,
  please report to the Source Control @ FB group.

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
  created new head
  $ hg smartlog -T '{rev}: {desc}\n'
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
  Run `hg pushbackup` to perform a backup.  If this fails,
  please report to the Source Control @ FB group.

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
  $ echo fakelock > .hg/infinitepushbackup.lock
  $ hg log -T '{if(backingup,"Yes","No")}\n' -r .
  Yes
  $ rm -f .hg/infinitepushbackup.lock

