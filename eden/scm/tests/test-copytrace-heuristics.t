#chg-compatible
#debugruntest-compatible

  $ configure mutation-norecord
  $ enable rebase shelve

Test for the heuristic copytracing algorithm
============================================

  $ initclient() {
  >   setconfig experimental.copytrace=heuristics experimental.copytrace.sourcecommitlimit=-1
  > }

NOTE: calling initclient() set copytrace.sourcecommitlimit=-1 as we want to
prevent the full copytrace algorithm to run and test the heuristic algorithm
without complexing the test cases with public and draft commits.

Check filename heuristics (same dirname and same basename)
----------------------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ mkdir dir
  $ echo a > dir/file.txt
  $ hg addremove
  adding a
  adding dir/file.txt
  $ hg ci -m initial
  $ hg mv a b
  $ hg mv -q dir dir2
  $ hg ci -m 'mv a b, mv dir/ dir2/'
  $ hg up -q 'desc(initial)'
  $ echo b > a
  $ echo b > dir/file.txt
  $ hg ci -qm 'mod a, mod dir/file.txt'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod a, mod dir/file.txt
  │
  │ o  desc: mv a b, mv dir/ dir2/
  ├─╯
  o  desc: initial
  

  $ hg rebase -s . -d 'desc(mv)'
  rebasing * "mod a, mod dir/file.txt" (glob)
  merging b and a to b
  merging dir2/file.txt and dir/file.txt to dir2/file.txt
  $ cd ..
  $ rm -rf repo

Make sure filename heuristics do not when they are not related
--------------------------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo 'somecontent' > a
  $ hg add a
  $ hg ci -m initial
  $ hg rm a
  $ echo 'completelydifferentcontext' > b
  $ hg add b
  $ hg ci -m 'rm a, add b'
  $ hg up -q 'desc(initial)'
  $ printf 'somecontent\nmoarcontent' > a
  $ hg ci -qm 'mode a'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mode a
  │
  │ o  desc: rm a, add b
  ├─╯
  o  desc: initial
  

  $ hg rebase -s . -d 'desc(rm)'
  rebasing * "mode a" (glob)
  other [source] changed a which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ cd ..
  $ rm -rf repo

Test when lca didn't modified the file that was moved
-----------------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo 'somecontent' > a
  $ hg add a
  $ hg ci -m initial
  $ echo c > c
  $ hg add c
  $ hg ci -m randomcommit
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg up -q 'desc(randomcommit)'
  $ echo b > a
  $ hg ci -qm 'mod a'

  $ hg log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mod a, phase: draft
  │
  │ o  desc: mv a b, phase: draft
  ├─╯
  o  desc: randomcommit, phase: draft
  │
  o  desc: initial, phase: draft
  

  $ hg rebase -s . -d 'desc(mv)'
  rebasing * "mod a" (glob)
  merging b and a to b
  $ cd ..
  $ rm -rf repo

Rebase "backwards"
------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo 'somecontent' > a
  $ hg add a
  $ hg ci -m initial
  $ echo c > c
  $ hg add c
  $ hg ci -m randomcommit
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg up -q 'desc(mv)'
  $ echo b > b
  $ hg ci -qm 'mod b'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod b
  │
  o  desc: mv a b
  │
  o  desc: randomcommit
  │
  o  desc: initial
  

  $ hg rebase -s . -d 'desc(initial)'
  rebasing * "mod b" (glob)
  merging a and b to a
  $ cd ..
  $ rm -rf repo

Check a few potential move candidates
-------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ mkdir dir
  $ echo a > dir/a
  $ hg add dir/a
  $ hg ci -qm initial
  $ hg mv dir/a dir/b
  $ hg ci -qm 'mv dir/a dir/b'
  $ mkdir dir2
  $ echo b > dir2/a
  $ hg add dir2/a
  $ hg ci -qm 'create dir2/a'
  $ hg up -q 'desc(initial)'
  $ echo b > dir/a
  $ hg ci -qm 'mod dir/a'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod dir/a
  │
  │ o  desc: create dir2/a
  │ │
  │ o  desc: mv dir/a dir/b
  ├─╯
  o  desc: initial
  

  $ hg rebase -s . -d 'desc(create)'
  rebasing * "mod dir/a" (glob)
  merging dir/b and dir/a to dir/b
  $ cd ..
  $ rm -rf repo

Test the copytrace.movecandidateslimit with many move candidates
----------------------------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a foo
  $ echo a > b
  $ echo a > c
  $ echo a > d
  $ echo a > e
  $ echo a > f
  $ echo a > g
  $ hg add b
  $ hg add c
  $ hg add d
  $ hg add e
  $ hg add f
  $ hg add g
  $ hg ci -m 'mv a foo, add many files'
  $ hg up -q ".^"
  $ echo b > a
  $ hg ci -m 'mod a'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: mv a foo, add many files
  ├─╯
  o  desc: initial
  

With small limit

  $ hg rebase -s 'desc(mod)' -d 'desc(mv)' --config experimental.copytrace.movecandidateslimit=0
  rebasing * "mod a" (glob)
  skipping copytracing for 'a', more candidates than the limit: 7
  other [source] changed a which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg rebase --abort
  rebase aborted

With default limit which is 100

  $ hg rebase -s 'desc(mod)' -d 'desc(mv)'
  rebasing * "mod a" (glob)
  merging foo and a to foo

  $ cd ..
  $ rm -rf repo

Move file in one branch and delete it in another
-----------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg up -q ".^"
  $ hg rm a
  $ hg ci -m 'del a'

  $ hg log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: del a, phase: draft
  │
  │ o  desc: mv a b, phase: draft
  ├─╯
  o  desc: initial, phase: draft
  

  $ hg rebase -s 'desc(mv)' -d 'desc(del)'
  rebasing * "mv a b" (glob)
  $ hg up -q c492ed3c7e35dcd1dc938053b8adf56e2cfbd062
  $ ls
  b
  $ cd ..
  $ rm -rf repo

Move a directory in draft branch
--------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ mkdir dir
  $ echo a > dir/a
  $ hg add dir/a
  $ hg ci -qm initial
  $ echo b > dir/a
  $ hg ci -qm 'mod dir/a'
  $ hg up -q ".^"
  $ hg mv -q dir/ dir2
  $ hg ci -qm 'mv dir/ dir2/'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mv dir/ dir2/
  │
  │ o  desc: mod dir/a
  ├─╯
  o  desc: initial
  

  $ hg rebase -s . -d 'desc(mod)'
  rebasing * "mv dir/ dir2/" (glob)
  merging dir/a and dir2/a to dir2/a
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file twice and rebase mod on top of moves
----------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg mv b c
  $ hg ci -m 'mv b c'
  $ hg up -q 'desc(initial)'
  $ echo c > a
  $ hg ci -m 'mod a'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: mv b c
  │ │
  │ o  desc: mv a b
  ├─╯
  o  desc: initial
  
  $ hg rebase -s . -d 'max(desc(mv))'
  rebasing * "mod a" (glob)
  merging c and a to c

  $ cd ..
  $ rm -rf repo

Move file twice and rebase moves on top of mods
-----------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg mv b c
  $ hg ci -m 'mv b c'
  $ hg up -q 'desc(initial)'
  $ echo c > a
  $ hg ci -m 'mod a'
  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: mv b c
  │ │
  │ o  desc: mv a b
  ├─╯
  o  desc: initial
  
  $ hg rebase -s 472e38d57782172f6c6abed82a94ca0d998c3a22 -d .
  rebasing * "mv a b" (glob)
  merging a and b to b
  rebasing * "mv b c" (glob)
  merging b and c to c

  $ cd ..
  $ rm -rf repo

Move one file and add another file in the same folder in one branch, modify file in another branch
--------------------------------------------------------------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ echo c > c
  $ hg add c
  $ hg ci -m 'add c'
  $ hg up -q 'desc(initial)'
  $ echo b > a
  $ hg ci -m 'mod a'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: add c
  │ │
  │ o  desc: mv a b
  ├─╯
  o  desc: initial
  

  $ hg rebase -s . -d 'desc(add)'
  rebasing * "mod a" (glob)
  merging b and a to b
  $ ls
  b
  c
  $ cat b
  b
  $ rm -rf repo

Merge test
----------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ echo b > a
  $ hg ci -m 'modify a'
  $ hg up -q 'desc(initial)'
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg up -q 'desc(mv)'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mv a b
  │
  │ o  desc: modify a
  ├─╯
  o  desc: initial
  

  $ hg merge 'desc(modify)'
  merging b and a to b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ ls
  b
  $ cd ..
  $ rm -rf repo

Copy and move file
------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg cp a c
  $ hg mv a b
  $ hg ci -m 'cp a c, mv a b'
  $ hg up -q 'desc(initial)'
  $ echo b > a
  $ hg ci -m 'mod a'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: cp a c, mv a b
  ├─╯
  o  desc: initial
  

  $ hg rebase -s . -d 'desc(cp)'
  rebasing * "mod a" (glob)
  merging b and a to b
  merging c and a to c
  $ ls
  b
  c
  $ cat b
  b
  $ cat c
  b
  $ cd ..
  $ rm -rf repo

Do a merge commit with many consequent moves in one branch
----------------------------------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ echo b > a
  $ hg ci -qm 'mod a'
  $ hg up -q ".^"
  $ hg mv a b
  $ hg ci -qm 'mv a b'
  $ hg mv b c
  $ hg ci -qm 'mv b c'
  $ hg up -q 'desc(mod)'
  $ hg log -G -T 'desc: {desc}\n'
  o  desc: mv b c
  │
  o  desc: mv a b
  │
  │ @  desc: mod a
  ├─╯
  o  desc: initial
  

  $ hg merge 'max(desc(mv))'
  merging a and c to c
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -qm 'merge'
  $ hg log -G -T 'desc: {desc}, phase: {phase}\n'
  @    desc: merge, phase: draft
  ├─╮
  │ o  desc: mv b c, phase: draft
  │ │
  │ o  desc: mv a b, phase: draft
  │ │
  o │  desc: mod a, phase: draft
  ├─╯
  o  desc: initial, phase: draft
  
  $ ls
  c
  $ cd ..
  $ rm -rf repo

Test shelve/unshelve
-------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ echo b > a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv a b
  $ hg ci -m 'mv a b'

  $ hg log -G -T 'desc: {desc}\n'
  @  desc: mv a b
  │
  o  desc: initial
  
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing * "shelve changes to: initial" (glob)
  merging b and a to b
  $ ls
  b
  $ cat b
  b
  $ cd ..
  $ rm -rf repo

Test full copytrace ability on draft branch
-------------------------------------------

File directory and base name changed in same move
  $ hg init repo
  $ initclient repo
  $ mkdir repo/dir1
  $ cd repo/dir1
  $ echo a > a
  $ hg add a
  $ hg ci -qm initial
  $ cd ..
  $ hg mv -q dir1 dir2
  $ hg mv dir2/a dir2/b
  $ hg ci -qm 'mv a b; mv dir1 dir2'
  $ hg up -q '.^'
  $ cd dir1
  $ echo b >> a
  $ cd ..
  $ hg ci -qm 'mod a'

  $ hg log -G -T 'desc {desc}, phase: {phase}\n'
  @  desc mod a, phase: draft
  │
  │ o  desc mv a b; mv dir1 dir2, phase: draft
  ├─╯
  o  desc initial, phase: draft
  

  $ hg rebase -s . -d 'desc(mv)' --config experimental.copytrace.sourcecommitlimit=100
  rebasing * "mod a" (glob)
  merging dir2/b and dir1/a to dir2/b
  $ cat dir2/b
  a
  b
  $ cd ..
  $ rm -rf repo

Move directory in one merge parent, while adding file to original directory
in other merge parent. File moved on rebase.

  $ hg init repo
  $ initclient repo
  $ mkdir repo/dir1
  $ cd repo/dir1
  $ echo dummy > dummy
  $ hg add dummy
  $ cd ..
  $ hg ci -qm initial
  $ cd dir1
  $ echo a > a
  $ hg add a
  $ cd ..
  $ hg ci -qm 'hg add dir1/a'
  $ hg up -q '.^'
  $ hg mv -q dir1 dir2
  $ hg ci -qm 'mv dir1 dir2'

  $ hg log -G -T 'desc {desc}, phase: {phase}\n'
  @  desc mv dir1 dir2, phase: draft
  │
  │ o  desc hg add dir1/a, phase: draft
  ├─╯
  o  desc initial, phase: draft
  

  $ hg rebase -s . -d 'desc(hg)' --config experimental.copytrace.sourcecommitlimit=100
  rebasing * "mv dir1 dir2" (glob)
  $ ls dir2
  a
  dummy
  $ rm -rf repo

Testing the sourcecommitlimit config
-----------------------------------

  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo a > a
  $ hg ci -Aqm "added a"
  $ echo "more things" >> a
  $ hg ci -qm "added more things to a"
  $ hg up 9092f1db7931481f93b37d5c9fbcfc341bcd7318
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ hg ci -Aqm "added b"
  $ mkdir foo
  $ hg mv a foo/bar
  $ hg ci -m "Moved a to foo/bar"
  $ hg log -G -T 'desc {desc}, phase: {phase}\n'
  @  desc Moved a to foo/bar, phase: draft
  │
  o  desc added b, phase: draft
  │
  │ o  desc added more things to a, phase: draft
  ├─╯
  o  desc added a, phase: draft
  

When the sourcecommitlimit is small and we have more drafts, we use heuristics only

  $ hg rebase -s 8b6e13696 -d .
  rebasing * "added more things to a" (glob)
  other [source] changed a which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

But when we have "sourcecommitlimit > (no. of drafts from base to c1)", we do
fullcopytracing

  $ hg rebase --abort
  rebase aborted
  $ hg rebase -s 8b6e13696 -d . --config experimental.copytrace.sourcecommitlimit=100
  rebasing * "added more things to a" (glob)
  merging foo/bar and a to foo/bar
  $ cd ..
  $ rm -rf repo
