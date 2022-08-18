#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ configure dummyssh
  $ hg init repo
  $ cd repo
  $ echo foo > foo
  $ hg ci -qAm 'add foo'
  $ echo >> foo
  $ hg ci -m 'change foo'
  $ hg up -qC 'desc(add)'
  $ echo bar > bar
  $ hg ci -qAm 'add bar'

  $ hg log
  commit:      effea6de0384
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  commit:      ed1b79f46b9a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo
  
  commit:      bbd179dfa0a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  $ cd ..

don't show "(+1 heads)" message when pulling closed head

  $ hg clone -q repo repo2
  $ hg clone -q repo2 repo3
  $ cd repo2
  $ hg up -q bbd179dfa0a71671c253b3ae0aa1513b60d199fa
  $ echo hello >> foo
  $ hg ci -mx1
  $ hg ci -mx2 --config ui.allowemptycommit=1
  $ cd ../repo3
  $ hg heads -q --closed
  effea6de0384
  ed1b79f46b9a
  $ hg pull
  pulling from $TESTTMP/repo2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg heads -q --closed
  1a1aa123db21
  effea6de0384
  ed1b79f46b9a

  $ cd ..

  $ hg init copy
  $ cd copy

Pull a missing revision:

  $ hg pull -qr missing ../repo
  abort: unknown revision 'missing'!
  [255]

Pull multiple revisions with update:

  $ cp -R . $TESTTMP/copy1
  $ cd $TESTTMP/copy1
  $ hg pull -qu -r 0 -r 1 ../repo
  $ hg -q parents
  bbd179dfa0a7

  $ cd $TESTTMP/copy
  $ hg pull -qr 0 ../repo
  $ hg log
  commit:      bbd179dfa0a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  $ hg pull -qr 1 ../repo
  $ hg log
  commit:      ed1b79f46b9a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo
  
  commit:      bbd179dfa0a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  

This used to abort: received changelog group is empty:

  $ hg pull -qr 1 ../repo
