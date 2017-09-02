Test for the heuristic copytracing algorithm
============================================

  $ cat >> $TESTTMP/copytrace.sh << '__EOF__'
  > initclient() {
  > cat >> $1/.hg/hgrc <<EOF
  > [experimental]
  > copytrace = heuristics
  > EOF
  > }
  > __EOF__
  $ . "$TESTTMP/copytrace.sh"

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > shelve=
  > EOF

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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 0
  $ echo b > a
  $ echo b > dir/file.txt
  $ hg ci -qm 'mod a, mod dir/file.txt'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 557f403c0afd2a3cf15d7e2fb1f1001a8b85e081
  |   desc: mod a, mod dir/file.txt, phase: draft
  | o  changeset: 928d74bc9110681920854d845c06959f6dfc9547
  |/    desc: mv a b, mv dir/ dir2/, phase: public
  o  changeset: 3c482b16e54596fed340d05ffaf155f156cda7ee
      desc: initial, phase: public

  $ hg rebase -s . -d 1
  rebasing 2:557f403c0afd "mod a, mod dir/file.txt" (tip)
  merging b and a to b
  merging dir2/file.txt and dir/file.txt to dir2/file.txt
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/557f403c0afd-9926eeff-rebase.hg (glob)
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 0
  $ printf 'somecontent\nmoarcontent' > a
  $ hg ci -qm 'mode a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d526312210b9e8f795d576a77dc643796384d86e
  |   desc: mode a, phase: draft
  | o  changeset: 46985f76c7e5e5123433527f5c8526806145650b
  |/    desc: rm a, add b, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ hg rebase -s . -d 1
  rebasing 2:d526312210b9 "mode a" (tip)
  other [source] changed a which local [dest] deleted
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 1
  $ echo b > a
  $ hg ci -qm 'mod a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 9d5cf99c3d9f8e8b05ba55421f7f56530cfcf3bc
  |   desc: mod a, phase: draft
  | o  changeset: d760186dd240fc47b91eb9f0b58b0002aaeef95d
  |/    desc: mv a b, phase: public
  o  changeset: 48e1b6ba639d5d7fb313fa7989eebabf99c9eb83
  |   desc: randomcommit, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ hg rebase -s . -d 2
  rebasing 3:9d5cf99c3d9f "mod a" (tip)
  merging b and a to b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/9d5cf99c3d9f-f02358cc-rebase.hg (glob)
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 2
  $ echo b > b
  $ hg ci -qm 'mod b'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: fbe97126b3969056795c462a67d93faf13e4d298
  |   desc: mod b, phase: draft
  o  changeset: d760186dd240fc47b91eb9f0b58b0002aaeef95d
  |   desc: mv a b, phase: public
  o  changeset: 48e1b6ba639d5d7fb313fa7989eebabf99c9eb83
  |   desc: randomcommit, phase: public
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: public

  $ hg rebase -s . -d 0
  rebasing 3:fbe97126b396 "mod b" (tip)
  merging a and b to a
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/fbe97126b396-cf5452a1-rebase.hg (glob)
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
  |   desc: mod a, phase: draft
  | o  changeset: 542cb58df733ee48fa74729bd2cdb94c9310d362
  |/    desc: mv a b, phase: draft
  o  changeset: e5b71fb099c29d9172ef4a23485aaffd497e4cc0
      desc: initial, phase: draft

  $ hg rebase -s . -d 1
  rebasing 2:5268f05aa168 "mod a" (tip)
  merging b and a to b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/5268f05aa168-284f6515-rebase.hg (glob)
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 0
  $ echo b > dir/a
  $ hg ci -qm 'mod dir/a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 6b2f4cece40fd320f41229f23821256ffc08efea
  |   desc: mod dir/a, phase: draft
  | o  changeset: 4494bf7efd2e0dfdd388e767fb913a8a3731e3fa
  | |   desc: create dir2/a, phase: public
  | o  changeset: b1784dfab6ea6bfafeb11c0ac50a2981b0fe6ade
  |/    desc: mv dir/a dir/b, phase: public
  o  changeset: 36859b8907c513a3a87ae34ba5b1e7eea8c20944
      desc: initial, phase: public

  $ hg rebase -s . -d 2
  rebasing 3:6b2f4cece40f "mod dir/a" (tip)
  merging dir/b and dir/a to dir/b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/6b2f4cece40f-503efe60-rebase.hg (glob)
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg up -q ".^"
  $ hg rm a
  $ hg ci -m 'del a'
  created new head

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 7d61ee3b1e48577891a072024968428ba465c47b
  |   desc: del a, phase: draft
  | o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  |/    desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg rebase -s 1 -d 2
  rebasing 1:472e38d57782 "mv a b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/472e38d57782-17d50e29-rebase.hg (glob)
  $ hg up -q c492ed3c7e35dcd1dc938053b8adf56e2cfbd062
  $ ls
  b
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ echo b > dir/a
  $ hg ci -qm 'mod dir/a'
  $ hg up -q ".^"
  $ hg mv -q dir/ dir2
  $ hg ci -qm 'mv dir/ dir2/'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: a33d80b6e352591dfd82784e1ad6cdd86b25a239
  |   desc: mv dir/ dir2/, phase: draft
  | o  changeset: 6b2f4cece40fd320f41229f23821256ffc08efea
  |/    desc: mod dir/a, phase: draft
  o  changeset: 36859b8907c513a3a87ae34ba5b1e7eea8c20944
      desc: initial, phase: public

  $ hg rebase -s . -d 1
  rebasing 2:a33d80b6e352 "mv dir/ dir2/" (tip)
  merging dir/a and dir2/a to dir2/a
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/a33d80b6e352-fecb9ada-rebase.hg (glob)
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 0
  $ echo c > a
  $ hg ci -m 'mod a'
  created new head
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d413169422167a3fa5275fc5d71f7dea9f5775f3
  |   desc: mod a, phase: draft
  | o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  | |   desc: mv b c, phase: public
  | o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  |/    desc: mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ hg rebase -s . -d 2
  rebasing 3:d41316942216 "mod a" (tip)
  merging c and a to c
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/d41316942216-2b5949bc-rebase.hg (glob)

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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg mv a b
  $ hg ci -m 'mv a b'
  $ hg mv b c
  $ hg ci -m 'mv b c'
  $ hg up -q 0
  $ echo c > a
  $ hg ci -m 'mod a'
  created new head
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: d413169422167a3fa5275fc5d71f7dea9f5775f3
  |   desc: mod a, phase: draft
  | o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  | |   desc: mv b c, phase: draft
  | o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  |/    desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ hg rebase -s 1 -d .
  rebasing 1:472e38d57782 "mv a b"
  merging a and b to b
  rebasing 2:d3efd280421d "mv b c"
  merging b and c to c
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/472e38d57782-ab8d3c58-rebase.hg (glob)

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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 0
  $ echo b > a
  $ hg ci -m 'mod a'
  created new head

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  |   desc: mod a, phase: draft
  | o  changeset: b1a6187e79fbce851bb584eadcb0cc4a80290fd9
  | |   desc: add c, phase: public
  | o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  |/    desc: mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg rebase -s . -d 2
  rebasing 3:ef716627c70b "mod a" (tip)
  merging b and a to b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/ef716627c70b-24681561-rebase.hg (glob)
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
  $ hg up -q 0
  $ hg mv a b
  $ hg ci -m 'mv a b'
  created new head
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 2

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  |   desc: mv a b, phase: public
  | o  changeset: b0357b07f79129a3d08a68621271ca1352ae8a09
  |/    desc: modify a, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg merge 1
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q 0
  $ echo b > a
  $ hg ci -m 'mod a'
  created new head

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  |   desc: mod a, phase: draft
  | o  changeset: 4fc3fd13fbdb89ada6b75bfcef3911a689a0dde8
  |/    desc: cp a c, mv a b, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg rebase -s . -d 1
  rebasing 2:ef716627c70b "mod a" (tip)
  merging b and a to b
  merging c and a to c
  saved backup bundle to $TESTTMP/repo/repo/.hg/strip-backup/ef716627c70b-24681561-rebase.hg (glob)
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
  $ cd ..
  $ hg clone -q server repo
  $ initclient repo
  $ cd repo
  $ hg up -q ".^"
  $ hg mv a b
  $ hg ci -qm 'mv a b'
  $ hg mv b c
  $ hg ci -qm 'mv b c'
  $ hg up -q 1
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  |   desc: mv b c, phase: draft
  o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  |   desc: mv a b, phase: draft
  | @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  |/    desc: mod a, phase: public
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public

  $ hg merge 3
  merging a and c to c
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -qm 'merge'
  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @    changeset: cd29b0d08c0f39bfed4cde1b40e30f419db0c825
  |\    desc: merge, phase: draft
  | o  changeset: d3efd280421d24f9f229997c19e654761c942a71
  | |   desc: mv b c, phase: draft
  | o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  | |   desc: mv a b, phase: draft
  o |  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  |/    desc: mod a, phase: public
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
  |   desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: public
  $ hg unshelve
  unshelving change 'default'
  rebasing shelved changes
  rebasing 2:45f63161acea "changes to: initial" (tip)
  merging b and a to b
  $ ls
  b
  $ cat b
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo
