  $ extpath=`dirname $TESTDIR`
  $ PYTHONPATH=$extpath:$TESTDIR/../:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastannotate=
  > EOF

  $ HGMERGE=true; export HGMERGE

  $ hg init repo
  $ cd repo

a simple merge case

  $ echo 1 > a
  $ hg commit -qAm 'append 1'
  $ echo 2 >> a
  $ hg commit -m 'append 2'
  $ echo 3 >> a
  $ hg commit -m 'append 3'
  $ hg up 1 -q
  $ cat > a << EOF
  > 0
  > 1
  > 2
  > EOF
  $ hg commit -qm 'insert 0'
  $ hg merge 2 -q
  $ echo 4 >> a
  $ hg commit -m merge
  $ hg log -G -T '{rev}: {desc}'
  @    4: merge
  |\
  | o  3: insert 0
  | |
  o |  2: append 3
  |/
  o  1: append 2
  |
  o  0: append 1
  
  $ hg fastannotate a
  3: 0
  0: 1
  1: 2
  2: 3
  4: 4
  $ hg fastannotate -r 0 a
  0: 1
  $ hg fastannotate -r 1 a
  0: 1
  1: 2
  $ hg fastannotate -udnclf a
  test 3 d641cb51f61e Thu Jan 01 00:00:00 1970 +0000 a:1: 0
  test 0 4994017376d3 Thu Jan 01 00:00:00 1970 +0000 a:1: 1
  test 1 e940cb6d9a06 Thu Jan 01 00:00:00 1970 +0000 a:2: 2
  test 2 26162a884ba6 Thu Jan 01 00:00:00 1970 +0000 a:3: 3
  test 4 3ad7bcd2815f Thu Jan 01 00:00:00 1970 +0000 a:5: 4
  $ hg fastannotate --linear a
  3: 0
  0: 1
  1: 2
  4: 3
  4: 4

incrementally updating

  $ hg fastannotate -r 0 a --debug
  fastannotate: a: no need to update linelog
  0: 1
  $ hg fastannotate -r 0 a --debug --rebuild
  fastannotate: a: 1 new changesets in the main branch
  0: 1
  $ hg fastannotate -r 1 a --debug
  fastannotate: a: 1 new changesets in the main branch
  0: 1
  1: 2
  $ hg fastannotate -r 3 a --debug
  fastannotate: a: 1 new changesets in the main branch
  3: 0
  0: 1
  1: 2
  $ hg fastannotate -r 4 a --debug
  fastannotate: a: 1 new changesets in the main branch
  3: 0
  0: 1
  1: 2
  2: 3
  4: 4
  $ hg fastannotate -r 1 a --debug
  fastannotate: a: no need to update linelog
  0: 1
  1: 2

rebuild happens automatically if unable to update

  $ hg fastannotate -r 2 a --debug
  fastannotate: a: cache broken and deleted
  fastannotate: a: 3 new changesets in the main branch
  0: 1
  1: 2
  2: 3

config option "fastannotate.mainbranch"

  $ hg fastannotate -r 1 --rebuild --config fastannotate.mainbranch=tip a --debug
  fastannotate: a: 4 new changesets in the main branch
  0: 1
  1: 2
  $ hg fastannotate -r 4 a --debug
  fastannotate: a: no need to update linelog
  3: 0
  0: 1
  1: 2
  2: 3
  4: 4

config option "fastannotate.commands"

  $ hg annotate -r 1 --debug a
  0: 1
  1: 2
  $ hg annotate --config fastannotate.commands=annotate -r 1 --debug a
  fastannotate: a: no need to update linelog
  0: 1
  1: 2
  $ hg fastannotate --config fastannotate.commands=annotate -h -q
  hg: unknown command 'fastannotate'
  (did you mean *) (glob)
  [255]

rename

  $ hg mv a b
  $ cat > b << EOF
  > 0
  > 11
  > 3
  > 44
  > EOF
  $ hg commit -m b -q
  $ hg fastannotate -ncf --long-hash b
  3 d641cb51f61e331c44654104301f8154d7865c89 a: 0
  5 d44dade239915bc82b91e4556b1257323f8e5824 b: 11
  2 26162a884ba60e8c87bf4e0d6bb8efcc6f711a4e a: 3
  5 d44dade239915bc82b91e4556b1257323f8e5824 b: 44
  $ hg fastannotate -r 26162a884ba60e8c87bf4e0d6bb8efcc6f711a4e a
  0: 1
  1: 2
  2: 3
