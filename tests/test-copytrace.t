  $ . "$TESTDIR/copytrace.sh"
  $ extpath=`dirname $TESTDIR`
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > copytrace=$extpath/hgext3rd/copytrace.py
  > rebase=
  > [experimental]
  > disablecopytrace=True
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
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/557f403c0afd-9926eeff-backup.hg (glob)
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
  hint: if this message is due to a moved file, you can ask mercurial to attempt to automatically resolve this change by re-running with the --tracecopies flag, but this will significantly slow down the operation, so you will need to be patient.
  Source control team is working on fixing this problem.
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
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/9d5cf99c3d9f-f02358cc-backup.hg (glob)
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
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/fbe97126b396-cf5452a1-backup.hg (glob)
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
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/5268f05aa168-284f6515-backup.hg (glob)
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Check a few potential move candidates
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
  $ hg up -q 0
  $ echo b > dir/a
  $ hg ci -qm 'mod dir/a'

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 6b2f4cece40fd320f41229f23821256ffc08efea
  |   desc: mod dir/a, phase: draft
  | o  changeset: 4494bf7efd2e0dfdd388e767fb913a8a3731e3fa
  | |   desc: create dir2/a, phase: draft
  | o  changeset: b1784dfab6ea6bfafeb11c0ac50a2981b0fe6ade
  |/    desc: mv dir/a dir/b, phase: draft
  o  changeset: 36859b8907c513a3a87ae34ba5b1e7eea8c20944
      desc: initial, phase: draft

  $ hg rebase -s . -d 2
  rebasing 3:6b2f4cece40f "mod dir/a" (tip)
  merging dir/b and dir/a to dir/b
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/6b2f4cece40f-503efe60-backup.hg (glob)
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Move file in one branch and delete it in another
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
  created new head

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: 7d61ee3b1e48577891a072024968428ba465c47b
  |   desc: del a, phase: draft
  | o  changeset: 472e38d57782172f6c6abed82a94ca0d998c3a22
  |/    desc: mv a b, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: draft

  $ hg rebase -s 1 -d 2
  rebasing 1:472e38d57782 "mv a b"
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/472e38d57782-17d50e29-backup.hg (glob)
  $ hg up -q c492ed3c7e35dcd1dc938053b8adf56e2cfbd062
  $ ls
  b
  $ cd ..
  $ rm -rf server
  $ rm -rf repo

Too many move candidates
  $ hg init repo
  $ initclient repo
  $ cd repo
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
  $ hg up -q ".^"
  $ echo b > a
  $ hg ci -m 'mod a'
  created new head

  $ hg log -G -T 'changeset: {node}\n desc: {desc}, phase: {phase}\n'
  @  changeset: ef716627c70bf4ca0bdb623cfb0d6fe5b9acc51e
  |   desc: mod a, phase: draft
  | o  changeset: d133babe0b735059c360d36b4b47200cdd6bcef5
  |/    desc: rm a, add many files, phase: draft
  o  changeset: 1451231c87572a7d3f92fc210b4b35711c949a98
      desc: initial, phase: draft

  $ hg rebase -s 2 -d 1
  rebasing 2:ef716627c70b "mod a" (tip)
  other [source] changed a which local [dest] deleted
  hint: if this message is due to a moved file, you can ask mercurial to attempt to automatically resolve this change by re-running with the --tracecopies flag, but this will significantly slow down the operation, so you will need to be patient.
  Source control team is working on fixing this problem.
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cd ..
  $ rm -rf server
  $ rm -rf repo
