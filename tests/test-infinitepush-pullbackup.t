
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
  $ hg pushbackup
  searching for changes
  remote: pushing 1 commit:
  remote:     89ecc969c0ac  firstcommit
  $ cd ..

Restore
  $ cd restored
  $ hg pullbackup
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
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
  searching for changes
  remote: pushing 1 commit:
  remote:     c1bfda8efb6e  secondcommit
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
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg log --graph -T '{desc}'
  o  secondcommit
  
  $ cd ..

Check bookmarks escaping
  $ cd backupsource
  $ hg book book/bookmarks/somebook
  $ hg book book/bookmarksbookmarks/somebook
  $ hg pushbackup
  $ cd ../restored
  $ hg pullbackup --reporoot $TESTTMP/backupsource
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
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
  searching for changes
  remote: pushing 1 commit:
  remote:     a2a9ae518b62  commitinweirdrepo
  $ cd ../../restored
  $ hg pullbackup --reporoot $TESTTMP/bookmarks/backupsource3
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  $ hg book
     abook                     1:89ecc969c0ac
     book/bookmarks/somebook   1:89ecc969c0ac
     book/bookmarksbookmarks/somebook 1:89ecc969c0ac
     bookbackupsource3         2:a2a9ae518b62
     secondbook                0:c1bfda8efb6e

  $ cd ..
