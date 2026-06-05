#chg-compatible

  $ eagerepo
  $ configure mutation-norecord
  $ enable rebase shelve

Test for the heuristic copytracing algorithm
============================================

Check filename heuristics (same dirname and same basename)
----------------------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ mkdir dir
  $ echo a > dir/file.txt
  $ sl addremove
  adding a
  adding dir/file.txt
  $ sl ci -m initial
  $ sl mv a b
  $ sl mv -q dir dir2
  $ sl ci -m 'mv a b, mv dir/ dir2/'
  $ sl up -q 'desc(initial)'
  $ echo b > a
  $ echo b > dir/file.txt
  $ sl ci -qm 'mod a, mod dir/file.txt'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod a, mod dir/file.txt
  │
  │ o  desc: mv a b, mv dir/ dir2/
  ├─╯
  o  desc: initial
  

  $ sl rebase -s . -d 'desc(mv)'
  rebasing * "mod a, mod dir/file.txt" (glob)
  merging b and a to b
  merging dir2/file.txt and dir/file.txt to dir2/file.txt
  $ cd ..
  $ rm -rf repo

Make sure filename heuristics do not when they are not related
--------------------------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo 'somecontent' > a
  $ sl add a
  $ sl ci -m initial
  $ sl rm a
  $ echo 'completelydifferentcontext' > b
  $ sl add b
  $ sl ci -m 'rm a, add b'
  $ sl up -q 'desc(initial)'
  $ printf 'somecontent\nmoarcontent' > a
  $ sl ci -qm 'mode a'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mode a
  │
  │ o  desc: rm a, add b
  ├─╯
  o  desc: initial
  

  $ sl rebase -s . -d 'desc(rm)'
  rebasing * "mode a" (glob)
  other [source] changed a which local [dest] is missing
  hint: the missing file was probably deleted by commit 46985f76c7e5 in the branch rebasing onto
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

  $ cd ..
  $ rm -rf repo

Test when lca didn't modified the file that was moved
-----------------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo 'somecontent' > a
  $ sl add a
  $ sl ci -m initial
  $ echo c > c
  $ sl add c
  $ sl ci -m randomcommit
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl up -q 'desc(randomcommit)'
  $ echo b > a
  $ sl ci -qm 'mod a'

  $ sl log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mod a, phase: draft
  │
  │ o  desc: mv a b, phase: draft
  ├─╯
  o  desc: randomcommit, phase: draft
  │
  o  desc: initial, phase: draft
  

  $ sl rebase -s . -d 'desc(mv)'
  rebasing * "mod a" (glob)
  merging b and a to b
  $ cd ..
  $ rm -rf repo

Rebase "backwards"
------------------

  $ sl init repo
  $ cd repo
  $ echo 'somecontent' > a
  $ sl add a
  $ sl ci -m initial
  $ echo c > c
  $ sl add c
  $ sl ci -m randomcommit
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl up -q 'desc(mv)'
  $ echo b > b
  $ sl ci -qm 'mod b'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod b
  │
  o  desc: mv a b
  │
  o  desc: randomcommit
  │
  o  desc: initial
  

  $ sl rebase -s . -d 'desc(initial)'
  rebasing * "mod b" (glob)
  merging a and b to a
  $ cd ..
  $ rm -rf repo

Check a few potential move candidates
-------------------------------------

  $ sl init repo
  $ cd repo
  $ mkdir dir
  $ echo a > dir/a
  $ sl add dir/a
  $ sl ci -qm initial
  $ sl mv dir/a dir/b
  $ sl ci -qm 'mv dir/a dir/b'
  $ mkdir dir2
  $ echo b > dir2/a
  $ sl add dir2/a
  $ sl ci -qm 'create dir2/a'
  $ sl up -q 'desc(initial)'
  $ echo b > dir/a
  $ sl ci -qm 'mod dir/a'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod dir/a
  │
  │ o  desc: create dir2/a
  │ │
  │ o  desc: mv dir/a dir/b
  ├─╯
  o  desc: initial
  

  $ sl rebase -s . -d 'desc(create)'
  rebasing * "mod dir/a" (glob)
  merging dir/b and dir/a to dir/b
  $ cd ..
  $ rm -rf repo

Test the copytrace.movecandidateslimit with many move candidates
----------------------------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a foo
  $ echo a > b
  $ echo a > c
  $ echo a > d
  $ echo a > e
  $ echo a > f
  $ echo a > g
  $ sl add b
  $ sl add c
  $ sl add d
  $ sl add e
  $ sl add f
  $ sl add g
  $ sl ci -m 'mv a foo, add many files'
  $ sl up -q ".^"
  $ echo b > a
  $ sl ci -m 'mod a'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: mv a foo, add many files
  ├─╯
  o  desc: initial
  

With small limit

  $ sl rebase -s 'desc(mod)' -d 'desc(mv)' --config copytrace.max-rename-candidates=0
  rebasing * "mod a" (glob)
  other [source] changed a which local [dest] is missing
  hint: the missing file was probably deleted by commit 8329d5c6bf47 in the branch rebasing onto
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

  $ sl rebase --abort
  rebase aborted

With default limit which is 100

  $ sl rebase -s 'desc(mod)' -d 'desc(mv)'
  rebasing * "mod a" (glob)
  merging foo and a to foo

  $ cd ..
  $ rm -rf repo

Move file in one branch and delete it in another
-----------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl up -q ".^"
  $ sl rm a
  $ sl ci -m 'del a'

  $ sl log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: del a, phase: draft
  │
  │ o  desc: mv a b, phase: draft
  ├─╯
  o  desc: initial, phase: draft
  

  $ sl rebase -s 'desc(mv)' -d 'desc(del)'
  rebasing * "mv a b" (glob)
  $ sl up -q c492ed3c7e35dcd1dc938053b8adf56e2cfbd062
  $ ls
  b
  $ cd ..
  $ rm -rf repo

Move a directory in draft branch
--------------------------------

  $ sl init repo
  $ cd repo
  $ mkdir dir
  $ echo a > dir/a
  $ sl add dir/a
  $ sl ci -qm initial
  $ echo b > dir/a
  $ sl ci -qm 'mod dir/a'
  $ sl up -q ".^"
  $ sl mv -q dir/ dir2
  $ sl ci -qm 'mv dir/ dir2/'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mv dir/ dir2/
  │
  │ o  desc: mod dir/a
  ├─╯
  o  desc: initial
  

  $ sl rebase -s . -d 'desc(mod)'
  rebasing * "mv dir/ dir2/" (glob)
  merging dir/a and dir2/a to dir2/a
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file twice and rebase mod on top of moves
----------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl mv b c
  $ sl ci -m 'mv b c'
  $ sl up -q 'desc(initial)'
  $ echo c > a
  $ sl ci -m 'mod a'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: mv b c
  │ │
  │ o  desc: mv a b
  ├─╯
  o  desc: initial
  
  $ sl rebase -s . -d 'max(desc(mv))'
  rebasing * "mod a" (glob)
  merging c and a to c

  $ cd ..
  $ rm -rf repo

Move file twice and rebase moves on top of mods
-----------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl mv b c
  $ sl ci -m 'mv b c'
  $ sl up -q 'desc(initial)'
  $ echo c > a
  $ sl ci -m 'mod a'
  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: mv b c
  │ │
  │ o  desc: mv a b
  ├─╯
  o  desc: initial
  
  $ sl rebase -s 472e38d57782172f6c6abed82a94ca0d998c3a22 -d .
  rebasing * "mv a b" (glob)
  merging a and b to b
  rebasing * "mv b c" (glob)
  merging b and c to c

  $ cd ..
  $ rm -rf repo

Move one file and add another file in the same folder in one branch, modify file in another branch
--------------------------------------------------------------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ echo c > c
  $ sl add c
  $ sl ci -m 'add c'
  $ sl up -q 'desc(initial)'
  $ echo b > a
  $ sl ci -m 'mod a'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: add c
  │ │
  │ o  desc: mv a b
  ├─╯
  o  desc: initial
  

  $ sl rebase -s . -d 'desc(add)'
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

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ echo b > a
  $ sl ci -m 'modify a'
  $ sl up -q 'desc(initial)'
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl up -q 'desc(mv)'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mv a b
  │
  │ o  desc: modify a
  ├─╯
  o  desc: initial
  

  $ sl merge 'desc(modify)'
  merging b and a to b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl ci -m merge
  $ ls
  b
  $ cd ..
  $ rm -rf repo

Copy and move file
------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl cp a c
  $ sl mv a b
  $ sl ci -m 'cp a c, mv a b'
  $ sl up -q 'desc(initial)'
  $ echo b > a
  $ sl ci -m 'mod a'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mod a
  │
  │ o  desc: cp a c, mv a b
  ├─╯
  o  desc: initial
  

  $ sl rebase -s . -d 'desc(cp)'
  rebasing * "mod a" (glob)
  merging b and a to b
  $ ls
  b
  c
  $ cat b
  b
  $ cat c
  a
  $ cd ..
  $ rm -rf repo

Do a merge commit with many consequent moves in one branch
----------------------------------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ echo b > a
  $ sl ci -qm 'mod a'
  $ sl up -q ".^"
  $ sl mv a b
  $ sl ci -qm 'mv a b'
  $ sl mv b c
  $ sl ci -qm 'mv b c'
  $ sl up -q 'desc(mod)'
  $ sl log -G -T 'desc: {desc}\n'
  o  desc: mv b c
  │
  o  desc: mv a b
  │
  │ @  desc: mod a
  ├─╯
  o  desc: initial
  

  $ sl merge 'max(desc(mv))'
  merging a and c to c
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl ci -qm 'merge'
  $ sl log -G -T 'desc: {desc}, phase: {phase}\n'
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

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ echo b > a
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl mv a b
  $ sl ci -m 'mv a b'

  $ sl log -G -T 'desc: {desc}\n'
  @  desc: mv a b
  │
  o  desc: initial
  
  $ sl unshelve
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
  $ sl init repo
  $ mkdir repo/dir1
  $ cd repo/dir1
  $ echo a > a
  $ sl add a
  $ sl ci -qm initial
  $ cd ..
  $ sl mv -q dir1 dir2
  $ sl mv dir2/a dir2/b
  $ sl ci -qm 'mv a b; mv dir1 dir2'
  $ sl up -q '.^'
  $ cd dir1
  $ echo b >> a
  $ cd ..
  $ sl ci -qm 'mod a'

  $ sl log -G -T 'desc {desc}, phase: {phase}\n'
  @  desc mod a, phase: draft
  │
  │ o  desc mv a b; mv dir1 dir2, phase: draft
  ├─╯
  o  desc initial, phase: draft
  

  $ sl rebase -s . -d 'desc(mv)' --config copytrace.sourcecommitlimit=100
  rebasing * "mod a" (glob)
  merging dir2/b and dir1/a to dir2/b
  $ cat dir2/b
  a
  b
  $ cd ..
  $ rm -rf repo

Move directory in one merge parent, while adding file to original directory
in other merge parent. File moved on rebase.

  $ sl init repo
  $ mkdir repo/dir1
  $ cd repo/dir1
  $ echo dummy > dummy
  $ sl add dummy
  $ cd ..
  $ sl ci -qm initial
  $ cd dir1
  $ echo a > a
  $ sl add a
  $ cd ..
  $ sl ci -qm 'sl add dir1/a'
  $ sl up -q '.^'
  $ sl mv -q dir1 dir2
  $ sl ci -qm 'mv dir1 dir2'

  $ sl log -G -T 'desc {desc}, phase: {phase}\n'
  @  desc mv dir1 dir2, phase: draft
  │
  │ o  desc sl add dir1/a, phase: draft
  ├─╯
  o  desc initial, phase: draft
  

  $ sl rebase -s . -d 'desc(sl)' --config copytrace.sourcecommitlimit=100
  rebasing * "mv dir1 dir2" (glob)
  $ ls dir2
  dummy
  $ rm -rf repo

Testing the sourcecommitlimit config
-----------------------------------

  $ sl init repo
  $ cd repo
  $ echo a > a
  $ sl ci -Aqm "added a"
  $ echo "more things" >> a
  $ sl ci -qm "added more things to a"
  $ sl up 9092f1db7931481f93b37d5c9fbcfc341bcd7318
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ sl ci -Aqm "added b"
  $ mkdir foo
  $ sl mv a foo/bar
  $ sl ci -m "Moved a to foo/bar"
  $ sl log -G -T 'desc {desc}, phase: {phase}\n'
  @  desc Moved a to foo/bar, phase: draft
  │
  o  desc added b, phase: draft
  │
  │ o  desc added more things to a, phase: draft
  ├─╯
  o  desc added a, phase: draft
  

When the sourcecommitlimit is small and we have more drafts, we use heuristics only

  $ sl rebase -s 8b6e13696 -d . --config copytrace.sourcecommitlimit=-1
  rebasing * "added more things to a" (glob)
  other [source] changed a which local [dest] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]

But when we have "sourcecommitlimit > (no. of drafts from base to c1)", we do
fullcopytracing

  $ sl rebase --abort
  rebase aborted
  $ sl rebase -s 8b6e13696 -d . --config copytrace.sourcecommitlimit=100
  rebasing 8b6e13696c38 "added more things to a"
  merging foo/bar and a to foo/bar
  $ cd ..
  $ rm -rf repo
