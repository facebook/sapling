
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Create backup source
  $ hg clone ssh://user@dummy/repo backupsource -q

Create restore target
  $ hg clone ssh://user@dummy/repo restored -q

Backup
  $ cd backupsource
  $ mkcommit firstcommit
  $ hg book abook

Actually do a backup, make sure that backup check doesn't fail for empty backup state
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     89ecc969c0ac  firstcommit
  finished in \d+\.(\d+)? seconds (re)
  $ cd ..

Restore
  $ cd restored
  $ hg pullbackup
  pulling from ssh://user@dummy/repo
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg log --graph -T '{desc}'
  o  firstcommit
  
  $ hg book
     abook                     0:89ecc969c0ac
  $ cd ..

Create second backup source
  $ hg clone ssh://user@dummy/repo backupsource2 -q
  $ cd backupsource2
  $ mkcommit secondcommit
  $ hg book secondbook
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     c1bfda8efb6e  secondcommit
  finished in \d+\.(\d+)? seconds (re)
  $ cd ..

Restore with ambiguous repo root
  $ rm -rf restored
  $ hg clone ssh://user@dummy/repo restored -q
  $ cd restored
  $ hg pullbackup
  abort: ambiguous repo root to restore: ['$TESTTMP/backupsource', '$TESTTMP/backupsource2']
  (set --reporoot to disambiguate)
  [255]
  $ hg pullbackup --reporoot $TESTTMP/backupsource2
  pulling from ssh://user@dummy/repo
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg log --graph -T '{desc}'
  o  secondcommit
  
  $ cd ..

Check bookmarks escaping
  $ cd backupsource
  $ hg book book/bookmarks/somebook
  $ hg book book/bookmarksbookmarks/somebook
  $ hg pushbackup
  starting backup .* (re)
  finished in \d+\.(\d+)? seconds (re)
  $ cd ../restored
  $ hg pullbackup --reporoot $TESTTMP/backupsource
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg book
     abook                     1:89ecc969c0ac
     book/bookmarks/somebook   1:89ecc969c0ac
     book/bookmarksbookmarks/somebook 1:89ecc969c0ac
     secondbook                0:c1bfda8efb6e
  $ cd ..

Create a repo with `/bookmarks/` in path
  $ mkdir bookmarks
  $ cd bookmarks
  $ hg clone ssh://user@dummy/repo backupsource3 -q
  $ cd backupsource3
  $ mkcommit commitinweirdrepo
  $ hg book bookbackupsource3
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     a2a9ae518b62  commitinweirdrepo
  finished in \d+\.(\d+)? seconds (re)
  $ cd ../../restored
  $ hg pullbackup --reporoot $TESTTMP/bookmarks/backupsource3
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg book
     abook                     1:89ecc969c0ac
     book/bookmarks/somebook   1:89ecc969c0ac
     book/bookmarksbookmarks/somebook 1:89ecc969c0ac
     bookbackupsource3         2:a2a9ae518b62
     secondbook                0:c1bfda8efb6e

Check that correct path is used in pushbackup
  $ cd ../backupsource
  $ hg --config paths.default=badpath --config paths.anotherpath=ssh://user@dummy/repo pushbackup
  starting backup .* (re)
  abort: repository $TESTTMP/backupsource/badpath not found!
  [255]
  $ hg pushbackup anotherpath --config paths.default=badpath --config paths.anotherpath=ssh://user@dummy/repo
  starting backup .* (re)
  nothing to backup
  finished in \d+\.(\d+)? seconds (re)
  $ cd ../restored

Check that correct path is used in pullbackup
  $ hg pullbackup --config paths.default=badpath --config paths.anotherpath=ssh://user@dummy/repo --reporoot $TESTTMP/bookmarks/backupsource3
  abort: repository $TESTTMP/restored/badpath not found!
  [255]
  $ hg pullbackup anotherpath --config paths.default=badpath --config paths.anotherpath=ssh://user@dummy/repo --reporoot $TESTTMP/bookmarks/backupsource3
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 1 files

  $ cd ..

Backup and restore two commits
  $ cd backupsource
  $ mkcommit firstinbatch
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark book/bookmarksbookmarks/somebook)
  $ mkcommit secondinbatch
  created new head
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 3 commits:
  remote:     89ecc969c0ac  firstcommit
  remote:     33c1c9df81e9  firstinbatch
  remote:     0e1a088ff282  secondinbatch
  finished in \d+\.(\d+)? seconds (re)
  $ cd ../restored

Install server-side extension that will print message every time when bundlerepo
is created
  $ cd ../repo
  $ printf "\n[extensions]\nbundlerepologger=$TESTDIR/bundlerepologger.py" >> .hg/hgrc
  $ hg st
  $ cd ../restored

Pull the backup and check bundlerepo was created only once
  $ hg pullbackup --reporoot $TESTTMP/backupsource | grep 'creating bundlerepo'
  remote: creating bundlerepo
  $ cd ../repo
  $ printf "\n[extensions]\nbundlerepologger=!" >> .hg/hgrc
  $ cd ../restored

Make sure that commits were restored
  $ hg log -r '33c1c9df81e9 + 0e1a088ff282' > /dev/null

Backup as another user, then restore it
  $ cd ../backupsource
  $ mkcommit backupasanotheruser
  $ hg log -r . -T '{node}\n'
  e0230a60975b38a9014f098fb973199efd25c46f
  $ HGUSER=anotheruser hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 3 commits:
  remote:     89ecc969c0ac  firstcommit
  remote:     0e1a088ff282  secondinbatch
  remote:     e0230a60975b  backupasanotheruser
  finished in \d+\.(\d+)? seconds (re)
  $ cd ../restored

Make sure commit was pulled by checking that commit is present
  $ hg log -r e0230a60975b38a9014f098fb973199efd25c46f -T '{node}\n'
  abort: unknown revision 'e0230a60975b38a9014f098fb973199efd25c46f'!
  [255]
  $ hg pullbackup --user anotheruser --reporoot $TESTTMP/backupsource > /dev/null
  $ hg log -r tip -T '{node}\n'
  e0230a60975b38a9014f098fb973199efd25c46f

Test debugcheckbackup
  $ hg debugcheckbackup
  abort: ambiguous repo root to restore: ['$TESTTMP', '$TESTTMP/backupsource', '$TESTTMP/backupsource2', '$TESTTMP/bookmarks/backupsource3']
  (set --reporoot to disambiguate)
  [255]
  $ hg debugcheckbackup --user anotheruser --reporoot $TESTTMP/backupsource
  checking \$TESTTMP/backupsource on .* (re)
  $ hg debugcheckbackup --all | sort
  checking \$TESTTMP on .* (re)
  checking \$TESTTMP/backupsource on .* (re)
  checking \$TESTTMP/backupsource2 on .* (re)
  checking \$TESTTMP/bookmarks/backupsource3 on .* (re)
  $ rm ../repo/.hg/scratchbranches/index/nodemap/e0230a60975b38a9014f098fb973199efd25c46f
  $ hg debugcheckbackup --user anotheruser --reporoot $TESTTMP/backupsource
  checking \$TESTTMP/backupsource on .* (re)
  unknown revision 'e0230a60975b38a9014f098fb973199efd25c46f'
  [255]

Make another backup from backupsource2 and run `hg debugcheckbackup --all` again.
Make sure that both repos were checked even though check for one of them fails
  $ cd ../backupsource2
  $ mkcommit newcommit
  $ HGUSER=anotheruser hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     c1bfda8efb6e  secondcommit
  remote:     c03baa769a20  newcommit
  finished in \d+\.(\d+)? seconds (re)
  $ cd ../backupsource
  $ hg debugcheckbackup --user anotheruser --all | sort
  unknown revision 'e0230a60975b38a9014f098fb973199efd25c46f'
  checking \$TESTTMP/backupsource on .* (re)
  checking \$TESTTMP/backupsource2 on .* (re)

Test getavailablebackups command
  $ hg getavailablebackups
  user test has 4 available backups:
  \$TESTTMP on .* (re)
  \$TESTTMP/backupsource on .* (re)
  \$TESTTMP/backupsource2 on .* (re)
  \$TESTTMP/bookmarks/backupsource3 on .* (re)
  $ hg getavailablebackups --user anotheruser
  user anotheruser has 2 available backups:
  \$TESTTMP/backupsource on .* (re)
  \$TESTTMP/backupsource2 on .* (re)
  $ hg getavailablebackups --json
  {".*": \["\$TESTTMP", "\$TESTTMP/backupsource", "\$TESTTMP/backupsource2", "\$TESTTMP/bookmarks/backupsource3"\]} (re)
