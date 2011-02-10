  $ hg init repo
  $ cd repo
  $ echo foo > foo
  $ hg ci -qAm 'add foo'
  $ echo >> foo
  $ hg ci -m 'change foo'
  $ hg up -qC 0
  $ echo bar > bar
  $ hg ci -qAm 'add bar'

  $ hg log
  changeset:   2:effea6de0384
  tag:         tip
  parent:      0:bbd179dfa0a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add bar
  
  changeset:   1:ed1b79f46b9a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo
  
  changeset:   0:bbd179dfa0a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  $ cd ..
  $ hg init copy
  $ cd copy

Pull a missing revision:

  $ hg pull -qr missing ../repo
  abort: unknown revision 'missing'!
  [255]

Pull multiple revisions with update:

  $ hg pull -qu -r 0 -r 1 ../repo
  $ hg -q parents
  0:bbd179dfa0a7
  $ hg rollback
  repository tip rolled back to revision -1 (undo pull)
  working directory now based on revision -1

  $ hg pull -qr 0 ../repo
  $ hg log
  changeset:   0:bbd179dfa0a7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  
  $ hg pull -qr 1 ../repo
  $ hg log
  changeset:   1:ed1b79f46b9a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change foo
  
  changeset:   0:bbd179dfa0a7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  

This used to abort: received changelog group is empty:

  $ hg pull -qr 1 ../repo

