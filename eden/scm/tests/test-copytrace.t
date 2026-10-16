
#require no-eden

#testcases tree-store tree-manifest

#if tree-manifest
  $ setconfig experimental.pathhistory-treemanifest=true
#endif

  $ configure mutation-norecord
  $ enable rebase shelve
  $ setconfig copytrace.dagcopytrace=False

  $ initclient() {
  >   setconfig copytrace.dagcopytrace=True
  > }

Check filename heuristics (same dirname and same basename)
  $ sl init server
  $ cd server
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
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(initial)'
  $ echo b > a
  $ echo b > dir/file.txt
  $ sl ci -qm 'mod a, mod dir/file.txt'

  $ sl log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mod a, mod dir/file.txt, phase: draft
  │
  │ o  desc: mv a b, mv dir/ dir2/, phase: public
  ├─╯
  o  desc: initial, phase: public


  $ sl rebase -s . -d 'desc(mv)'
  rebasing * "mod a, mod dir/file.txt" (glob)
  merging b and a to b
  merging dir2/file.txt and dir/file.txt to dir2/file.txt
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Make sure filename heuristics do not when they are not related
  $ sl init server
  $ cd server
  $ echo 'somecontent' > a
  $ sl add a
  $ sl ci -m initial
  $ sl rm a
  $ echo 'completelydifferentcontext' > b
  $ sl add b
  $ sl ci -m 'rm a, add b'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(initial)'
  $ printf 'somecontent\nmoarcontent' > a
  $ sl ci -qm 'mode a'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d526312210b9e8f795d576a77dc643796384d86e
  │   desc: mode a, phase: draft
  │ o  changeset: 46985f76c7e5e5123433527f5c8526806145650b
  ├─╯   desc: rm a, add b, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ sl rebase -s . -d 'desc(rm)'
  rebasing d526312210b9 "mode a"
  other [source] changed a which local [dest] is missing
  hint: the missing file was probably deleted by commit 46985f76c7e5 in the branch rebasing onto
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Test when lca didn't modified the file that was moved
  $ sl init server
  $ cd server
  $ echo 'somecontent' > a
  $ sl add a
  $ sl ci -m initial
  $ echo c > c
  $ sl add c
  $ sl ci -m randomcommit
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(randomcommit)'
  $ echo b > a
  $ sl ci -qm 'mod a'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 9d5cf99c3d9f8e8b05ba55421f7f56530cfcf3bc
  │   desc: mod a, phase: draft
  │ o  changeset: d760186dd240fc47b91eb9f0b58b0002aaeef95d
  ├─╯   desc: mv a b, phase: public
  o  changeset: 48e1b6ba639d5d7fb313fa7989eebabf99c9eb83
  │   desc: randomcommit, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ sl rebase -s . -d 'desc(mv)'
  rebasing 9d5cf99c3d9f "mod a"
  merging b and a to b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Rebase "backwards"
  $ sl init server
  $ cd server
  $ echo 'somecontent' > a
  $ sl add a
  $ sl ci -m initial
  $ echo c > c
  $ sl add c
  $ sl ci -m randomcommit
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(mv)'
  $ echo b > b
  $ sl ci -qm 'mod b'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: fbe97126b3969056795c462a67d93faf13e4d298
  │   desc: mod b, phase: draft
  o  changeset: d760186dd240fc47b91eb9f0b58b0002aaeef95d
  │   desc: mv a b, phase: public
  o  changeset: 48e1b6ba639d5d7fb313fa7989eebabf99c9eb83
  │   desc: randomcommit, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ sl rebase -s . -d 'desc(initial)'
  rebasing fbe97126b396 "mod b"
  merging a and b to a
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Rebase draft commit on top of draft commit
  $ sl init repo
  $ initclient repo
  $ cd repo
  $ echo 'somecontent' > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl up -q ".^"
  $ echo b > a
  $ sl ci -qm 'mod a'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 5268f05aa1684cfb5741e9eb05eddcc1c5ee7508
  │   desc: mod a, phase: draft
  │ o  changeset: 542cb58df733ee48fa74729bd2cdb94c9310d362
  ├─╯   desc: mv a b, phase: draft
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: draft

  $ sl rebase -s . -d 'desc(mv)'
  rebasing 5268f05aa168 "mod a"
  merging b and a to b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Check a few potential move candidates
  $ sl init server
  $ initclient server
  $ cd server
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
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(initial)'
  $ echo b > dir/a
  $ sl ci -qm 'mod dir/a'

  $ sl log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mod dir/a, phase: draft
  │
  │ o  desc: create dir2/a, phase: public
  │ │
  │ o  desc: mv dir/a dir/b, phase: public
  ├─╯
  o  desc: initial, phase: public


  $ sl rebase -s . -d 'desc(create)'
  rebasing * "mod dir/a" (glob)
  merging dir/b and dir/a to dir/b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file in one branch and delete it in another
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl up -q ".^"
  $ sl rm a
  $ sl ci -m 'del a'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 7d61ee3b1e48577891a072024968428ba465c47b
  │   desc: del a, phase: draft
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ sl rebase -s 'desc(mv)' -d 'desc(del)'
  rebasing 472e38d57782 "mv a b"
  $ sl up -q c492ed3c7e35dcd1dc938053b8adf56e2cfbd062
  $ ls
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Too many move candidates
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl rm a
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
  $ sl ci -m 'rm a, add many files'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q ".^"
  $ echo b > a
  $ sl ci -m 'mod a'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  │   desc: mod a, phase: draft
  │ o  changeset: d133babe0b735059c360d36b4b47200cdd6bcef5
  ├─╯   desc: rm a, add many files, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ sl rebase -s 'desc(mod)' -d 'desc(rm)'
  rebasing ef716627c70b "mod a"
  other [source] changed a which local [dest] is missing
  hint: the missing file was probably deleted by commit d133babe0b73 in the branch rebasing onto
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move a directory in draft branch
  $ sl init server
  $ initclient server
  $ cd server
  $ mkdir dir
  $ echo a > dir/a
  $ sl add dir/a
  $ sl ci -qm initial
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ echo b > dir/a
  $ sl ci -qm 'mod dir/a'
  $ sl up -q ".^"
  $ sl mv -q dir/ dir2
  $ sl ci -qm 'mv dir/ dir2/'

  $ sl log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mv dir/ dir2/, phase: draft
  │
  │ o  desc: mod dir/a, phase: draft
  ├─╯
  o  desc: initial, phase: public


  $ sl rebase -s . -d 'desc(mod)'
  rebasing * "mv dir/ dir2/" (glob)
  merging dir/a and dir2/a to dir2/a
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file twice and rebase mod on top of moves
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl mv b c
  $ sl ci -m 'mv b c'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(initial)'
  $ echo c > a
  $ sl ci -m 'mod a'
  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d413169422167a3fa5275fc5d71f7dea9f5775f3
  │   desc: mod a, phase: draft
  │ o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  │ │   desc: mv b c, phase: public
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ sl rebase -s . -d 'max(desc(mv))'
  rebasing d41316942216 "mod a"
  merging c and a to c

  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file twice and rebase moves on top of mods
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl mv b c
  $ sl ci -m 'mv b c'
  $ sl up -q 'desc(initial)'
  $ echo c > a
  $ sl ci -m 'mod a'
  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d413169422167a3fa5275fc5d71f7dea9f5775f3
  │   desc: mod a, phase: draft
  │ o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  │ │   desc: mv b c, phase: draft
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ sl rebase -s 472e38d57782172f6c6abed82a94ca0d998c3a22 -d .
  rebasing 472e38d57782 "mv a b"
  merging a and b to b
  rebasing d3efd280421d "mv b c"
  merging b and c to c

  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move one file and add another file in the same folder in one branch, modify file in another branch
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ echo c > c
  $ sl add c
  $ sl ci -m 'add c'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(initial)'
  $ echo b > a
  $ sl ci -m 'mod a'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  │   desc: mod a, phase: draft
  │ o  changeset: b1a6187e79fbce851bb584eadcb0cc4a80290fd9
  │ │   desc: add c, phase: public
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ sl rebase -s . -d 'desc(add)'
  rebasing ef716627c70b "mod a"
  merging b and a to b
  $ ls
  b
  c
  $ cat b
  b

Merge test
  $ sl init server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ echo b > a
  $ sl ci -m 'modify a'
  $ sl bookmark book1
  $ sl up -q 'desc(initial)'
  $ sl mv a b
  $ sl ci -m 'mv a b'
  $ sl bookmark book2
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -B book2 -q
  $ sl up -q 'desc(mv)'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  │   desc: mv a b, phase: public
  │ o  changeset: b0357b07f79129a3d08a68621271ca1352ae8a09
  ├─╯   desc: modify a, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ sl merge 'desc(modify)'
  merging b and a to b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl ci -m merge
  $ ls
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Copy and move file
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl cp a c
  $ sl mv a b
  $ sl ci -m 'cp a c, mv a b'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q 'desc(initial)'
  $ echo b > a
  $ sl ci -m 'mod a'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  │   desc: mod a, phase: draft
  │ o  changeset: 4fc3fd13fbdb89ada6b75bfcef3911a689a0dde8
  ├─╯   desc: cp a c, mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

dagcopytrace does support copying & renaming one file to multiple files, it picks the first one in ascending order

  $ sl rebase -s . -d 'desc(cp)'
  rebasing ef716627c70b "mod a"
  merging b and a to b
  $ ls
  b
  c
  $ cat b
  b
  $ cat c
  a
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Do a merge commit with many consequent moves in one branch
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ echo b > a
  $ sl ci -qm 'mod a'
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ sl up -q ".^"
  $ sl mv a b
  $ sl ci -qm 'mv a b'
  $ sl mv b c
  $ sl ci -qm 'mv b c'
  $ sl up -q 'desc(mod)'
  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  │   desc: mv b c, phase: draft
  o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  │   desc: mv a b, phase: draft
  │ @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  ├─╯   desc: mod a, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ sl merge 'max(desc(mv))'
  merging a and c to c
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ sl ci -qm 'merge'
  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @    changeset: cd29b0d08c0f39bfed4cde1b40e30f419db0c825
  ├─╮   desc: merge, phase: draft
  │ o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  │ │   desc: mv b c, phase: draft
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  │ │   desc: mv a b, phase: draft
  o │  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  ├─╯   desc: mod a, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ ls
  c
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Test shelve/unshelve
  $ sl init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ sl add a
  $ sl ci -m initial
  $ sl bookmark book1
  $ cd ..
  $ sl clone -q server repo
  $ initclient repo
  $ cd repo
  $ sl pull -B book1 -q
  $ echo b > a
  $ sl shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl mv a b
  $ sl ci -m 'mv a b'

  $ sl log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  │   desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ sl unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing f0569b377759 "shelve changes to: initial"
  merging b and a to b
  $ ls
  b
  $ cat b
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Test full copytrace ability on draft branch: File directory and base name
changed in same move
  $ sl init repo
  $ initclient repo
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


  $ sl rebase -s . -d 'desc(mv)'
  rebasing * "mod a" (glob)
  merging dir2/b and dir1/a to dir2/b
  $ cat dir2/b
  a
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Test full copytrace ability on draft branch: Move directory in one merge parent,
while adding file to original directory in other merge parent. File moved on rebase.
  $ sl init repo
  $ initclient repo
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

# dagcopytrace does not track directory move
  $ sl rebase -s . -d 'desc(sl)'
  rebasing * "mv dir1 dir2" (glob)
  $ ls dir2
  dummy
  $ rm -rf server
  $ rm -rf repo
