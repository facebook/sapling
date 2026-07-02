#require mononoke

  $ . "$TESTDIR/library.sh"

  $ newserver master_bookmark

  $ clone master_bookmark client
  $ cd client
  $ mkdir dir
  $ echo x > dir/x
  $ sl commit -qAm x
  $ mkdir dir/ydir
  $ echo y > dir/ydir/y
  $ sl commit -qAm y
  $ sl rm dir/x
  $ sl rm dir/ydir/y
  $ sl commit -qAm rm
  $ sl push -q -r tip --to master_bookmark --create
  $ cd ..

Shallow clone

  $ clone master_bookmark shallow --noupdate
  $ cd shallow
  $ setconfig extensions.amend= rebase.experimental.inmemory=True

  $ sl up master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl up .~1
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ findfilessorted dir
  dir/x
  dir/ydir/y

  $ sl up .~1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ findfilessorted dir
  dir/x
  $ sl up master_bookmark
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ test -f dir
  [1]

  $ echo x > x
  $ sl commit -qAm x
  $ echo y > y
  $ sl commit -qAm y
  $ sl prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [02b5b1] x
  $ sl amend --date "1 1"
  rebasing 02663ae2e9f7 "y"
