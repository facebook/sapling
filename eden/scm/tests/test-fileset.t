#chg-compatible
#debugruntest-compatible

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ fileset() {
  >   hg debugfileset "$@"
  > }

  $ hg init repo
  $ cd repo
  $ echo a > a1
  $ echo a > a2
  $ echo b > b1
  $ echo b > b2
  $ hg ci -Am addfiles
  adding a1
  adding a2
  adding b1
  adding b2

Test operators and basic patterns

  $ fileset -v a1
  (symbol 'a1')
  a1
  $ fileset -v 'a*'
  (symbol 'a*')
  a1
  a2
  $ fileset -v '"re:a\\d"'
  (string 're:a\\d')
  a1
  a2
  $ fileset -v 'a1 or a2'
  (or
    (symbol 'a1')
    (symbol 'a2'))
  a1
  a2
  $ fileset 'a1 | a2'
  a1
  a2
  $ fileset 'a* and "*1"'
  a1
  $ fileset 'a* & "*1"'
  a1
  $ fileset 'not (r"a*")'
  b1
  b2
  $ fileset '! ("a*")'
  b1
  b2
  $ fileset 'a* - a1'
  a2
  $ fileset 'a_b'
  $ fileset '"\xy"'
  hg: parse error: invalid \x escape at position 0
  [255]

Test files status

  $ rm a1
  $ hg rm a2
  $ echo b >> b2
  $ hg cp b1 c1
  $ echo c > c2
  $ echo c > c3
  $ cat > .gitignore <<EOF
  > .gitignore
  > *2
  > EOF
  $ fileset 'modified()'
  b2
  $ fileset 'added()'
  c1
  $ fileset 'removed()'
  a2
  $ fileset 'deleted()'
  a1
  $ fileset 'missing()'
  a1
  $ fileset 'unknown()'
  c3
  $ fileset 'ignored()'
  .gitignore
  c2
  $ fileset 'gitignore()'
  a2
  b2
  $ fileset 'clean()'
  b1
  $ fileset 'copied()'
  c1

Test files status in different revisions

  $ hg status -m
  M b2
  $ fileset -r0 'revs("wdir()", modified())' --traceback
  b2
  $ hg status -a
  A c1
  $ fileset -r0 'revs("wdir()", added())'
  c1
  $ hg status --change 'desc(addfiles)' -a
  A a1
  A a2
  A b1
  A b2
  $ hg status -mru
  M b2
  R a2
  ? c3
  $ fileset -r0 'added() and revs("wdir()", modified() or removed() or unknown())'
  b2
  a2
  $ fileset -r0 'added() or revs("wdir()", added())'
  a1
  a2
  b1
  b2
  c1

Test files properties

  >>> _ = open('bin', 'wb').write(b'\0a')
  $ fileset 'binary()'
  $ fileset 'binary() and unknown()'
  bin
  $ echo 'bin' >> .gitignore
  $ fileset 'binary() and ignored()'
  bin
  $ hg add bin
  $ fileset 'binary()'
  bin

  $ fileset 'grep("b{1}")'
  b2
  c1
  b1
  $ fileset 'grep("missingparens(")'
  hg: parse error: invalid match pattern: missing ), unterminated subpattern at position 13
  [255]

#if execbit
  $ chmod +x b2
  $ fileset 'exec()'
  b2
#endif

#if symlink
  $ ln -s b2 b2link
  $ fileset 'symlink() and unknown()'
  b2link
  $ hg add b2link
#endif

#if no-windows
  $ echo foo > con.xml
  $ fileset 'not portable()'
  con.xml
  $ hg --config ui.portablefilenames=ignore add con.xml
#endif

  >>> _ = open('1k', 'wb').write(b' '*1024)
  >>> _ = open('2k', 'wb').write(b' '*2048)
  $ hg add 1k 2k
  $ fileset 'size("bar")'
  hg: parse error: couldn't parse size: bar
  [255]
  $ fileset '(1k, 2k)'
  hg: parse error: can't use a list in this context
  (see hg help "filesets.x or y")
  [255]
  $ fileset 'size(1k)'
  1k
  $ fileset '(1k or 2k) and size("< 2k")'
  1k
  $ fileset '(1k or 2k) and size("<=2k")'
  1k
  2k
  $ fileset '(1k or 2k) and size("> 1k")'
  2k
  $ fileset '(1k or 2k) and size(">=1K")'
  1k
  2k
  $ fileset '(1k or 2k) and size(".5KB - 1.5kB")'
  1k
  $ fileset 'size("1M")'
  $ fileset 'size("1 GB")'

Test merge states

  $ hg ci -m manychanges
  $ hg up -C 'desc(addfiles)'
  * files updated, 0 files merged, * files removed, 0 files unresolved (glob)
  $ echo c >> b2
  $ hg ci -m diverging b2
  $ fileset 'resolved()'
  $ fileset 'unresolved()'
  $ hg merge
  merging b2
  warning: 1 conflicts while merging b2! (edit, then use 'hg resolve --mark')
  * files updated, 0 files merged, 1 files removed, 1 files unresolved (glob)
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ fileset 'resolved()'
  $ fileset 'unresolved()'
  b2
  $ echo e > b2
  $ hg resolve -m b2
  (no more unresolved files)
  $ fileset 'resolved()'
  b2
  $ fileset 'unresolved()'
  $ hg ci -m merge

There was a commit from subrepo here. Now subrepos are gone, insert a dummy commit to take its place.

  $ hg commit -m 'subrepo' --config ui.allowemptycommit=1

Test with a revision


  $ hg log -G --template '{desc}\n'
  @  subrepo
  │
  o    merge
  ├─╮
  │ o  diverging
  │ │
  o │  manychanges
  ├─╯
  o  addfiles
  
  $ echo unknown > unknown
  $ fileset -r1 'modified()'
  b2
  $ fileset -r1 'added() and c1'
  c1
  $ fileset -r1 'removed()'
  a2
  $ fileset -r1 'deleted()'
  $ fileset -r1 'unknown()'
  $ fileset -r1 'ignored()'
  $ fileset -r1 'gitignore()'
  b2
  bin
  $ fileset -r1 'binary()'
  bin
  $ fileset -r1 'size(1k)'
  1k
  $ fileset -r3 'resolved()'
  $ fileset -r3 'unresolved()'

#if execbit
  $ fileset -r1 'exec()'
  b2
#endif

#if symlink
  $ fileset -r1 'symlink()'
  b2link
#endif

#if no-windows
  $ fileset -r1 'not portable()'
  con.xml
  $ hg forget 'con.xml'
#endif

  $ fileset -r4 'b2 or c1'
  b2
  c1

  >>> _ = open('dos', 'wb').write(b"dos\r\n")
  >>> _ = open('mixed', 'wb').write(b"dos\r\nunix\n")
  >>> _ = open('mac', 'wb').write(b"mac\r")
  $ hg add dos mixed mac

(remove a1, to examine safety of 'eol' on removed files)
  $ rm a1

  $ fileset 'eol(dos)'
  dos
  mixed
  $ fileset 'eol(unix)'
  mixed
  b1
  b2
  c1
  $ fileset 'eol(mac)'
  mac

Test safety of 'encoding' on removed files

  $ fileset 'encoding("ascii")'
  dos
  mac
  mixed
  1k
  2k
  b1
  b2
  b2link (symlink !)
  bin
  c1

Test detection of unintentional 'matchctx.existing()' invocation

  $ cat > $TESTTMP/existingcaller.py <<EOF
  > from edenscm import registrar
  > 
  > filesetpredicate = registrar.filesetpredicate()
  > @filesetpredicate('existingcaller()', callexisting=False)
  > def existingcaller(mctx, x):
  >     # this 'mctx.existing()' invocation is unintentional
  >     return [f for f in mctx.existing()]
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > existingcaller = $TESTTMP/existingcaller.py
  > EOF

  $ fileset 'existingcaller()' 2>&1 | tail -1
  AssertionError: unexpected existing() invocation

Test 'revs(...)'
================

small reminder of the repository state

  $ hg log -G
  @  commit:      * (glob)
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     subrepo
  │
  o    commit:      * (glob)
  ├─╮  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     merge
  │ │
  │ o  commit:      55b05bdebf36
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     diverging
  │ │
  o │  commit:      * (glob)
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     manychanges
  │
  o  commit:      8a9576c51c1f
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     addfiles
  
  $ hg status --change 'desc(addfiles)'
  A a1
  A a2
  A b1
  A b2
  $ hg status --change 'desc(manychanges)'
  M b2
  A 1k
  A 2k
  A b2link (no-windows !)
  A bin
  A c1
  A con.xml (no-windows !)
  R a2
  $ hg status --change 'desc(diverging)'
  M b2
  $ hg status --change 'desc(merge)'
  M b2
  A 1k
  A 2k
  A b2link (no-windows !)
  A bin
  A c1
  A con.xml (no-windows !)
  R a2
  $ hg status --change 'desc(subrepo)'
  $ hg status
  A dos
  A mac
  A mixed
  R con.xml (no-windows !)
  ! a1
  ? b2.orig
  ? c3
  ? unknown

Test files at -r0 should be filtered by files at wdir
-----------------------------------------------------

  $ fileset -r0 '* and revs("wdir()", *)'
  a1
  b1
  b2

Test that "revs()" work at all
------------------------------

  $ fileset "revs('2', modified())"
  b2

Test that "revs()" work for file missing in the working copy/current context
----------------------------------------------------------------------------

(a2 not in working copy)

  $ fileset "revs('0', added())"
  a1
  a2
  b1
  b2

(none of the file exist in "0")

  $ fileset -r 0 "revs('4', added())"

Call with empty revset
--------------------------

  $ fileset "revs('2-2', modified())"

Call with revset matching multiple revs
---------------------------------------

  $ fileset "revs('0+4', added())"
  a1
  a2
  b1
  b2

overlapping set

  $ fileset "revs('1+2', modified())"
  b2

test 'status(...)'
=================

Simple case
-----------

  $ fileset "status(3, 4, added())"

use rev to restrict matched file
-----------------------------------------

  $ hg status --removed --rev 0 --rev 1
  R a2
  $ fileset "status(0, 1, removed())"
  a2
  $ fileset "* and status(0, 1, removed())"
  $ fileset -r 4 "status(0, 1, removed())"
  a2
  $ fileset -r 4 "* and status(0, 1, removed())"
  $ fileset "revs('4', * and status(0, 1, removed()))"
  $ fileset "revs('0', * and status(0, 1, removed()))"
  a2

check wdir()
------------

  $ hg status --removed  --rev 'desc(subrepo)'
  R con.xml (no-windows !)
  $ fileset "status(4, 'wdir()', removed())"
  con.xml (no-windows !)

  $ hg status --removed --rev 'desc(diverging)'
  R a2
  $ fileset "status('2', 'wdir()', removed())"
  a2

test backward status
--------------------

  $ hg status --removed --rev 0 --rev 4
  R a2
  $ hg status --added --rev 4 --rev 0
  A a2
  $ fileset "status(4, 0, added())"
  a2

test cross branch status
------------------------

  $ hg status --added --rev 1 --rev 2
  A a2
  $ fileset "status(1, 2, added())"
  a2

test with multi revs revset
---------------------------
  $ hg status --added --rev 'desc(addfiles)':'desc(manychanges)' --rev 'desc(merge)':'desc(subrepo)'
  A 1k
  A 2k
  A b2link (no-windows !)
  A bin
  A c1
  A con.xml (no-windows !)
  $ fileset "status('0:1', '3:4', added())"
  1k
  2k
  b2link (no-windows !)
  bin
  c1
  con.xml (no-windows !)

tests with empty value
----------------------

Fully empty revset

  $ fileset "status('', '4', added())"
  hg: parse error: first argument to status must be a revision
  [255]
  $ fileset "status('2', '', added())"
  hg: parse error: second argument to status must be a revision
  [255]

Empty revset will error at the revset layer

  $ fileset "status(' ', '4', added())"
  hg: parse error at 1: not a prefix: end
  ( 
    ^ here)
  [255]
  $ fileset "status('2', ' ', added())"
  hg: parse error at 1: not a prefix: end
  ( 
    ^ here)
  [255]
