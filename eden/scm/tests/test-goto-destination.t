#require no-eden

  $ setconfig checkout.show-destination=true
  $ newclientrepo
  $ echo a > a
  $ sl commit -Aqm "first commit"
  $ FIRST=$(sl log -r . -T '{node}')
  $ echo b > b
  $ sl commit -Aqm "second commit"
  $ SECOND=$(sl log -r . -T '{node}')

Goto shows the destination hash and title:

  $ sl goto $FIRST
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  checked out * "first commit" (glob)

Revset destinations resolve through the Python implementation and still show it:

  $ sl goto 'desc("second commit")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checked out * "second commit" (glob)

Quiet suppresses the destination line:

  $ sl goto -q $FIRST

HGPLAIN suppresses the destination line:

  $ HGPLAIN=1 sl goto $SECOND
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

The config can disable the output:

  $ sl goto --config checkout.show-destination=false $FIRST
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

prev and next print their own destination line, not goto's:

  $ sl next
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  [*] second commit (glob)
