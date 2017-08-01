
  $ setup() {
  > cat << EOF >> .hg/hgrc
  > [extensions]
  > fbamend=$TESTDIR/../hgext3rd/fbamend
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
  $ hg push
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ echo a > file1
  $ changeset_time=`expr $now - 13 \* 60`
  $ hg commit -d "$commit_time 0" -m "Backed up changeset"
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     *  Backed up changeset (glob)
  finished in \d+\.(\d+)? seconds (re)
  $ echo b > file1
  $ commit_time=`expr $now - 11 \* 60`
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset"
  $ echo c > file1
  $ commit_time=`expr $now - 9 \* 60`
  $ hg commit -d "$commit_time 0" -m "Backup pending changeset"

Check backup status of commits
  $ hg log -T '{rev} {desc}\n' -r 'backedup()'
  1 Backed up changeset
  $ hg log -T '{rev} {desc}\n' -r 'draft() - backedup()'
  2 Not backed up changeset
  3 Backup pending changeset

Check smartlog output
  $ hg smartlog
  @  changeset:   3:* (glob)
  |  tag:         tip
  |  user:        test
  |  date:        * (glob)
  |  summary:     Backup pending changeset
  |
  o  changeset:   2:* (glob)
  |  user:        test
  |  date:        * (glob)
  |  summary:     Not backed up changeset
  |
  o  changeset:   1:* (glob)
  |  user:        test
  |  date:        * (glob)
  |  summary:     Backed up changeset
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
  @  3: Backup pending changeset
  |
  o  2: Not backed up changeset
  |
  o  1: Backed up changeset
  |
  o  0: Public changeset
  
Check smartlog summary with multiple unbacked up changesets
  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b2 > file1
  $ commit_time=`expr $now - 11 \* 60`
  $ hg commit -d "$commit_time 0" -m "Not backed up changeset 2"
  created new head
  $ hg smartlog -T '{rev}: {desc}\n'
  @  4: Not backed up changeset 2
  |
  | o  3: Backup pending changeset
  |/
  o  2: Not backed up changeset
  |
  o  1: Backed up changeset
  |
  o  0: Public changeset
  
  note: 2 changesets are not backed up.
  Run `hg pushbackup` to perform a backup.  If this fails,
  please report to the Source Control @ FB group.
