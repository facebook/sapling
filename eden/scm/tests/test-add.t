
#require no-eden


  $ eagerepo
  $ sl init a
  $ cd a
  $ echo a > a
  $ sl add -n
  adding a
  $ sl st
  ? a
  $ sl add
  adding a
  $ sl st
  A a
  $ sl forget a
  $ sl add
  adding a
  $ sl st
  A a
  $ mkdir dir
  $ cd dir
  $ sl add ../a
  ../a already tracked!
  $ cd ..

  $ echo b > b
  $ sl add -n b
  $ sl st
  A a
  ? b
  $ sl add b
  $ sl st
  A a
  A b

should fail

  $ sl add b
  b already tracked!
  $ sl st
  A a
  A b

#if no-windows
  $ echo foo > con.xml
  $ sl --config ui.portablefilenames=jump add con.xml
  abort: ui.portablefilenames value is invalid ('jump')
  [255]
  $ sl --config ui.portablefilenames=abort add con.xml
  abort: filename contains 'con', which is reserved on Windows: con.xml
  [255]
  $ sl st
  A a
  A b
  ? con.xml
  $ sl add con.xml
  warning: filename contains 'con', which is reserved on Windows: con.xml
  $ sl st
  A a
  A b
  A con.xml
  $ sl forget con.xml
  $ rm con.xml
#endif

#if eol-in-paths
  $ echo bla > 'hello:world'
  $ sl --config ui.portablefilenames=abort add
  adding hello:world
  abort: filename contains ':', which is reserved on Windows: 'hello:world'
  [255]
  $ sl st
  A a
  A b
  ? hello:world
  $ sl --config ui.portablefilenames=ignore add
  adding hello:world
  $ sl st
  A a
  A b
  A hello:world
#endif

  $ sl ci -m 0 --traceback

  $ sl log -r "heads(. or wdir() & file('**'))"
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
should fail

  $ sl add a
  a already tracked!

  $ echo aa > a
  $ sl ci -m 1
  $ sl up 'desc(0)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa > a
  $ sl ci -m 2

  $ sl merge
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'sl resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ sl st
  M a
  ? a.orig

wdir doesn't cause a crash, and can be dynamically selected if dirty
XXX: Rust revset backend drops "wdir()". Planned fix is to add virtual
nodes to the Rust commit graph on the fly.

  $ sl log -r "heads(. or wdir() & file('**'))"
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  

should fail

  $ sl add a
  a already tracked!
  $ sl st
  M a
  ? a.orig
  $ sl resolve -m a
  (no more unresolved files)
  $ sl ci -m merge

Issue683: peculiarity with hg revert of an removed then added file

  $ sl forget a
  $ sl add a
  $ sl st
  ? a.orig
  $ sl rm a
  $ sl st
  R a
  ? a.orig
  $ echo a > a
  $ sl add a
  $ sl st
  M a
  ? a.orig

Forgotten file can be added back (as either clean or modified)

  $ sl forget b
  $ sl add b
  $ sl st -A b
  C b
  $ sl forget b
  $ echo modified > b
  $ sl add b
  $ sl st -A b
  M b
  $ sl revert -qC b

  $ sl add c && echo "unexpected addition of missing file"
  c: * (glob)
  [1]
  $ echo c > c
  $ sl add d c && echo "unexpected addition of missing file"
  d: * (glob)
  [1]
  $ sl st
  M a
  A c
  ? a.orig
  $ sl up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

forget and get should have the right order: added but missing dir should be
forgotten before file with same name is added

  $ echo file d > d
  $ sl add d
  $ sl ci -md
  $ sl rm d
  $ mkdir d
  $ echo a > d/a
  $ sl add d/a
  $ rm -r d
  $ sl up -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat d
  file d

Test that adding a directory doesn't require case matching (issue4578)
#if icasefs
  $ mkdir -p CapsDir1/CapsDir
  $ echo abc > CapsDir1/CapsDir/AbC.txt
  $ mkdir CapsDir1/CapsDir/SubDir
  $ echo def > CapsDir1/CapsDir/SubDir/Def.txt

  $ sl add capsdir1/capsdir
  adding CapsDir1/CapsDir/AbC.txt
  adding CapsDir1/CapsDir/SubDir/Def.txt

  $ sl forget capsdir1/capsdir/abc.txt
  removing CapsDir1/CapsDir/AbC.txt

  $ sl forget capsdir1/capsdir
  removing CapsDir1/CapsDir/SubDir/Def.txt

  $ sl add capsdir1
  adding CapsDir1/CapsDir/AbC.txt
  adding CapsDir1/CapsDir/SubDir/Def.txt

  $ sl ci -m "AbCDef" capsdir1/capsdir

  $ sl status -A capsdir1/capsdir
  C CapsDir1/CapsDir/AbC.txt
  C CapsDir1/CapsDir/SubDir/Def.txt

  $ sl files capsdir1/capsdir
  CapsDir1/CapsDir/AbC.txt
  CapsDir1/CapsDir/SubDir/Def.txt

  $ echo xyz > CapsDir1/CapsDir/SubDir/Def.txt
  $ sl ci -m xyz capsdir1/capsdir/subdir/def.txt

  $ sl revert -r '.^' capsdir1/capsdir
  reverting CapsDir1/CapsDir/SubDir/Def.txt

The conditional tests above mean the hash on the diff line differs on Windows
and OS X
  $ sl diff capsdir1/capsdir
  diff -r * CapsDir1/CapsDir/SubDir/Def.txt (glob)
  --- a/CapsDir1/CapsDir/SubDir/Def.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/CapsDir1/CapsDir/SubDir/Def.txt	* (glob)
  @@ -1,1 +1,1 @@
  -xyz
  +def

  $ sl mv CapsDir1/CapsDir/abc.txt CapsDir1/CapsDir/ABC.txt
  moving CapsDir1/CapsDir/AbC.txt to CapsDir1/CapsDir/ABC.txt
  $ sl ci -m "case changing rename" CapsDir1/CapsDir/AbC.txt CapsDir1/CapsDir/ABC.txt

  $ sl status -A capsdir1/capsdir
  M CapsDir1/CapsDir/SubDir/Def.txt
  C CapsDir1/CapsDir/ABC.txt

  $ sl remove -f 'glob:**.txt' -X capsdir1/capsdir
  $ sl remove -f 'glob:**.txt' -I capsdir1/capsdir
  removing CapsDir1/CapsDir/ABC.txt
  removing CapsDir1/CapsDir/SubDir/Def.txt
#endif

  $ cd ..

Adding a file that matches a gitignore rule warns the user:

  $ newrepo ignored
  $ echo 'ignored.txt' > .gitignore
  $ echo 'ignored_dir/' >> .gitignore
  $ sl ci -m init -A .gitignore
  $ echo content > ignored.txt
  $ mkdir ignored_dir
  $ echo content > ignored_dir/a
  $ echo content > tracked.txt

Warning is emitted for explicitly added ignored file:

  $ sl add ignored.txt
  the following files are ignored, but still added because they are explicitly specified:
    ignored.txt
  (use 'sl debugignore <file>' to check why they are ignored)
  $ sl forget ignored.txt

Mixed add: warning lists only the ignored file:

  $ sl add ignored.txt ignored_dir/a tracked.txt
  the following files are ignored, but still added because they are explicitly specified:
    ignored.txt
    ignored_dir/a
  (use 'sl debugignore <file>' to check why they are ignored)
  $ sl status
  A ignored.txt
  A ignored_dir/a
  A tracked.txt

The warning is silenced by -q:

  $ sl forget ignored.txt ignored_dir/a tracked.txt
  $ sl add -q ignored.txt ignored_dir/a tracked.txt
  $ sl status
  A ignored.txt
  A ignored_dir/a
  A tracked.txt
