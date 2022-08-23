#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig experimental.allowfilepeer=True

  $ configure mutation-norecord
  $ enable copytrace rebase shelve remotenames
  $ setconfig experimental.copytrace=off

  $ initclient() {
  >   setconfig copytrace.remote=false copytrace.enablefilldb=true copytrace.fastcopytrace=true
  >   setconfig experimental.copytrace=off
  > }

Check filename heuristics (same dirname and same basename)
  $ hg init server
  $ cd server
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
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(initial)'
  $ echo b > a
  $ echo b > dir/file.txt
  $ hg ci -qm 'mod a, mod dir/file.txt'

  $ hg log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mod a, mod dir/file.txt, phase: draft
  │
  │ o  desc: mv a b, mv dir/ dir2/, phase: public
  ├─╯
  o  desc: initial, phase: public
  

  $ hg rebase -s . -d 'desc(mv)'
  rebasing * "mod a, mod dir/file.txt" (glob)
  merging b and a to b
  merging dir2/file.txt and dir/file.txt to dir2/file.txt
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Make sure filename heuristics do not when they are not related
  $ hg init server
  $ cd server
  $ echo 'somecontent' > a
  $ hg add a
  $ hg ci -m initial
  $ hg rm a
  $ echo 'completelydifferentcontext' > b
  $ hg add b
  $ hg ci -m 'rm a, add b'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(initial)'
  $ printf 'somecontent\nmoarcontent' > a
  $ hg ci -qm 'mode a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d526312210b9e8f795d576a77dc643796384d86e
  │   desc: mode a, phase: draft
  │ o  changeset: 46985f76c7e5e5123433527f5c8526806145650b
  ├─╯   desc: rm a, add b, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ hg rebase -s . -d 'desc(rm)'
  rebasing d526312210b9 "mode a"
  other [source] changed a which local [dest] deleted
  hint: if this is due to a renamed file, you can manually input the renamed path, or re-run the command using --config=experimental.copytrace=on to make hg figure out renamed path automatically (which is very slow, and you will need to be patient)
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Test when lca didn't modified the file that was moved
  $ hg init server
  $ cd server
  $ echo 'somecontent' > a
  $ hg add a
  $ hg ci -m initial
  $ echo c > c
  $ hg add c
  $ hg ci -m randomcommit
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(randomcommit)'
  $ echo b > a
  $ hg ci -qm 'mod a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 9d5cf99c3d9f8e8b05ba55421f7f56530cfcf3bc
  │   desc: mod a, phase: draft
  │ o  changeset: d760186dd240fc47b91eb9f0b58b0002aaeef95d
  ├─╯   desc: mv a b, phase: public
  o  changeset: 48e1b6ba639d5d7fb313fa7989eebabf99c9eb83
  │   desc: randomcommit, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ hg rebase -s . -d 'desc(mv)'
  rebasing 9d5cf99c3d9f "mod a"
  merging b and a to b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Rebase "backwards"
  $ hg init server
  $ cd server
  $ echo 'somecontent' > a
  $ hg add a
  $ hg ci -m initial
  $ echo c > c
  $ hg add c
  $ hg ci -m randomcommit
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(mv)'
  $ echo b > b
  $ hg ci -qm 'mod b'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: fbe97126b3969056795c462a67d93faf13e4d298
  │   desc: mod b, phase: draft
  o  changeset: d760186dd240fc47b91eb9f0b58b0002aaeef95d
  │   desc: mv a b, phase: public
  o  changeset: 48e1b6ba639d5d7fb313fa7989eebabf99c9eb83
  │   desc: randomcommit, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ hg rebase -s . -d 'desc(initial)'
  rebasing fbe97126b396 "mod b"
  merging a and b to a
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Rebase draft commit on top of draft commit
  $ hg init repo
  $ initclient repo
  $ cd repo
  $ echo 'somecontent' > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg up -q ".^"
  $ echo b > a
  $ hg ci -qm 'mod a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 5268f05aa1684cfb5741e9eb05eddcc1c5ee7508
  │   desc: mod a, phase: draft
  │ o  changeset: 542cb58df733ee48fa74729bd2cdb94c9310d362
  ├─╯   desc: mv a b, phase: draft
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: draft

  $ hg rebase -s . -d 'desc(mv)'
  rebasing 5268f05aa168 "mod a"
  merging b and a to b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Check a few potential move candidates
  $ hg init server
  $ initclient server
  $ cd server
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
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(initial)'
  $ echo b > dir/a
  $ hg ci -qm 'mod dir/a'

  $ hg log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mod dir/a, phase: draft
  │
  │ o  desc: create dir2/a, phase: public
  │ │
  │ o  desc: mv dir/a dir/b, phase: public
  ├─╯
  o  desc: initial, phase: public
  

  $ hg rebase -s . -d 'desc(create)'
  rebasing * "mod dir/a" (glob)
  merging dir/b and dir/a to dir/b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file in one branch and delete it in another
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg up -q ".^"
  $ hg rm a
  $ hg ci -m 'del a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 7d61ee3b1e48577891a072024968428ba465c47b
  │   desc: del a, phase: draft
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg rebase -s 'desc(mv)' -d 'desc(del)'
  rebasing 472e38d57782 "mv a b"
  $ hg up -q c492ed3c7e35dcd1dc938053b8adf56e2cfbd062
  $ ls
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Too many move candidates
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg rm a
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
  $ hg ci -m 'rm a, add many files'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q ".^"
  $ echo b > a
  $ hg ci -m 'mod a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  │   desc: mod a, phase: draft
  │ o  changeset: d133babe0b735059c360d36b4b47200cdd6bcef5
  ├─╯   desc: rm a, add many files, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg rebase -s 'desc(mod)' -d 'desc(rm)'
  rebasing ef716627c70b "mod a"
  other [source] changed a which local [dest] deleted
  hint: if this is due to a renamed file, you can manually input the renamed path, or re-run the command using --config=experimental.copytrace=on to make hg figure out renamed path automatically (which is very slow, and you will need to be patient)
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move a directory in draft branch
  $ hg init server
  $ initclient server
  $ cd server
  $ mkdir dir
  $ echo a > dir/a
  $ hg add dir/a
  $ hg ci -qm initial
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ echo b > dir/a
  $ hg ci -qm 'mod dir/a'
  $ hg up -q ".^"
  $ hg mv -q dir/ dir2
  $ hg ci -qm 'mv dir/ dir2/'

  $ hg log -G -T 'desc: {desc}, phase: {phase}\n'
  @  desc: mv dir/ dir2/, phase: draft
  │
  │ o  desc: mod dir/a, phase: draft
  ├─╯
  o  desc: initial, phase: public
  

  $ hg rebase -s . -d 'desc(mod)'
  rebasing * "mv dir/ dir2/" (glob)
  merging dir/a and dir2/a to dir2/a
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file twice and rebase mod on top of moves
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg mv b c
  $ hg ci -m 'mv b c'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(initial)'
  $ echo c > a
  $ hg ci -m 'mod a'
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d413169422167a3fa5275fc5d71f7dea9f5775f3
  │   desc: mod a, phase: draft
  │ o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  │ │   desc: mv b c, phase: public
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ hg rebase -s . -d 'max(desc(mv))'
  rebasing d41316942216 "mod a"
  merging c and a to c

  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file twice and rebase moves on top of mods
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg mv b c
  $ hg ci -m 'mv b c'
  $ hg up -q 'desc(initial)'
  $ echo c > a
  $ hg ci -m 'mod a'
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d413169422167a3fa5275fc5d71f7dea9f5775f3
  │   desc: mod a, phase: draft
  │ o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  │ │   desc: mv b c, phase: draft
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ hg rebase -s 472e38d57782172f6c6abed82a94ca0d998c3a22 -d .
  rebasing 472e38d57782 "mv a b"
  merging a and b to b
  rebasing d3efd280421d "mv b c"
  merging b and c to c

  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move one file and add another file in the same folder in one branch, modify file in another branch
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ echo c > c
  $ hg add c
  $ hg ci -m 'add c'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(initial)'
  $ echo b > a
  $ hg ci -m 'mod a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  │   desc: mod a, phase: draft
  │ o  changeset: b1a6187e79fbce851bb584eadcb0cc4a80290fd9
  │ │   desc: add c, phase: public
  │ o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  ├─╯   desc: mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg rebase -s . -d 'desc(add)'
  rebasing ef716627c70b "mod a"
  merging b and a to b
  $ ls
  b
  c
  $ cat b
  b

Merge test
  $ hg init server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ echo b > a
  $ hg ci -m 'modify a'
  $ hg bookmark book1
  $ hg up -q 'desc(initial)'
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg bookmark book2
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(mv)'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  │   desc: mv a b, phase: public
  │ o  changeset: b0357b07f79129a3d08a68621271ca1352ae8a09
  ├─╯   desc: modify a, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg merge 'desc(modify)'
  merging b and a to b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ ls
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Copy and move file
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg cp a c
  $ hg mv a b
  $ hg ci -m 'cp a c, mv a b'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 'desc(initial)'
  $ echo b > a
  $ hg ci -m 'mod a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  │   desc: mod a, phase: draft
  │ o  changeset: 4fc3fd13fbdb89ada6b75bfcef3911a689a0dde8
  ├─╯   desc: cp a c, mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg rebase -s . -d 'desc(cp)'
  rebasing ef716627c70b "mod a"
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
  $ rm -rf server
  $ rm -rf repo

Do a merge commit with many consequent moves in one branch
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ echo b > a
  $ hg ci -qm 'mod a'
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q ".^"
  $ hg mv a b
  $ hg ci -qm 'mv a b'
  $ hg mv b c
  $ hg ci -qm 'mv b c'
  $ hg up -q 'desc(mod)'
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  │   desc: mv b c, phase: draft
  o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  │   desc: mv a b, phase: draft
  │ @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  ├─╯   desc: mod a, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg merge 'max(desc(mv))'
  merging a and c to c
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -qm 'merge'
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
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
  $ hg init server
  $ initclient server
  $ cd server
  $ echo a > a
  $ hg add a
  $ hg ci -m initial
  $ hg bookmark book1
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ echo b > a
  $ hg shelve
  shelved as default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv a b
  $ hg ci -m 'mv a b'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  │   desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ hg unshelve
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
  

  $ hg rebase -s . -d 'desc(mv)' --config=experimental.copytrace=on
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
  

  $ hg rebase -s . -d 'desc(hg)' --config=experimental.copytrace=on
  rebasing * "mv dir1 dir2" (glob)
  $ ls dir2
  a
  dummy
  $ rm -rf server
  $ rm -rf repo
