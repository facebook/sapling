#chg-compatible

  $ setconfig extensions.treemanifest=!
Setup


  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > strip =
  > EOF

Setup pushrebase required repo

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > [pushrebase]
  > blocknonpushrebase = True
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ cd ..

  $ hg clone -q server client
  $ cd client
  $ echo b >> a && hg commit -Aqm b
  $ hg book master

Non-pushrebase pushes should be rejected

  $ hg push
  pushing to $TESTTMP/server (glob)
  searching for changes
  error: prechangegroup.blocknonpushrebase hook failed: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  abort: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  [255]

  $ hg push -f
  pushing to $TESTTMP/server (glob)
  searching for changes
  error: prechangegroup.blocknonpushrebase hook failed: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  abort: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  [255]

  $ hg push -B master
  pushing to $TESTTMP/server (glob)
  searching for changes
  error: prechangegroup.blocknonpushrebase hook failed: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  abort: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  [255]

Pushrebase pushes should be allowed

  $ hg push --config "extensions.pushrebase=" --to master -B master
  pushing to $TESTTMP/server (glob)
  searching for changes
  pushing 1 changeset:
      1846eede8b68  b
  exporting bookmark master

Bookmark pushes should not be affected by the block

  $ hg book -r ".^" master -f
  $ hg push -B master
  pushing to $TESTTMP/server (glob)
  searching for changes
  no changes found
  updating bookmark master
  [1]
  $ hg -R ../server log -T '{rev} {bookmarks}' -G
  o  1
  |
  @  0 master
  
