Create a repo, set the username to something more than 255 bytes, then run hg amend on it.

  $ unset HGUSER
  $ cat >> $HGRCPATH << EOF
  > [ui]
  > username = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa <very.long.name@example.com>
  > [extensions]
  > amend =
  > [experimental]
  > evolution.createmarkers=True
  > evolution.exchange=True
  > EOF
  $ hg init tmpa
  $ cd tmpa
  $ echo a > a
  $ hg add
  adding a
  $ hg commit -m "Initial commit"
  $ echo a >> a
  $ hg amend 2>&1 | egrep -v '^(\*\*|  )'
  transaction abort!
  rollback completed
  Traceback (most recent call last):
  *ProgrammingError: obsstore metadata value cannot be longer than 255 bytes (value "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa <very.long.name@example.com>" for key "user" is 285 bytes) (glob)
