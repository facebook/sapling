  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > show =
  > EOF

  $ hg init repo0
  $ cd repo0

Empty repo / no checkout results in error

  $ hg show stack
  abort: stack view only available when there is a working directory
  [255]

Stack displays single draft changeset as root revision

  $ echo 0 > foo
  $ hg -q commit -A -m 'commit 0'
  $ hg show stack
    @  9f17 commit 0

Stack displays multiple draft changesets

  $ echo 1 > foo
  $ hg commit -m 'commit 1'
  $ echo 2 > foo
  $ hg commit -m 'commit 2'
  $ echo 3 > foo
  $ hg commit -m 'commit 3'
  $ echo 4 > foo
  $ hg commit -m 'commit 4'
  $ hg show stack
    @  2737 commit 4
    o  d1a6 commit 3
    o  128c commit 2
    o  181c commit 1
    o  9f17 commit 0

Public parent of draft base is displayed, separated from stack

  $ hg phase --public -r 0
  $ hg show stack
    @  2737 commit 4
    o  d1a6 commit 3
    o  128c commit 2
    o  181c commit 1
   /   (stack base)
  o  9f17 commit 0

  $ hg phase --public -r 1
  $ hg show stack
    @  2737 commit 4
    o  d1a6 commit 3
    o  128c commit 2
   /   (stack base)
  o  181c commit 1

Draft descendants are shown

  $ hg -q up 2
  $ hg show stack
    o  2737 commit 4
    o  d1a6 commit 3
    @  128c commit 2
   /   (stack base)
  o  181c commit 1

  $ hg -q up 3
  $ hg show stack
    o  2737 commit 4
    @  d1a6 commit 3
    o  128c commit 2
   /   (stack base)
  o  181c commit 1

working dir on public changeset should display special message

  $ hg -q up 1
  $ hg show stack
  (empty stack; working directory parent is a published changeset)

Branch point in descendants displayed at top of graph

  $ hg -q up 3
  $ echo b > foo
  $ hg commit -m 'commit 5 (new dag branch)'
  created new head
  $ hg -q up 2
  $ hg show stack
   \ /  (multiple children)
    |
    o  d1a6 commit 3
    @  128c commit 2
   /   (stack base)
  o  181c commit 1

  $ cd ..

Base is stopped at merges

  $ hg init merge-base
  $ cd merge-base
  $ echo 0 > foo
  $ hg -q commit -A -m initial
  $ echo h1 > foo
  $ hg commit -m 'head 1'
  $ hg -q up 0
  $ echo h2 > foo
  $ hg -q commit -m 'head 2'
  $ hg phase --public -r 0:tip
  $ hg -q up 1
  $ hg merge -t :local 2
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m 'merge heads'

TODO doesn't yet handle case where wdir is a draft merge

  $ hg show stack
    @  8ee9 merge heads
   /   (stack base)
  o  5947 head 1

  $ echo d1 > foo
  $ hg commit -m 'draft 1'
  $ echo d2 > foo
  $ hg commit -m 'draft 2'

  $ hg show stack
    @  430d draft 2
    o  787b draft 1
   /   (stack base)
  o  8ee9 merge heads

  $ cd ..

Now move on to stacks when there are more commits after the base branchpoint

  $ hg init public-rebase
  $ cd public-rebase
  $ echo 0 > foo
  $ hg -q commit -A -m 'base'
  $ hg phase --public -r .
  $ echo d1 > foo
  $ hg commit -m 'draft 1'
  $ echo d2 > foo
  $ hg commit -m 'draft 2'
  $ hg -q up 0
  $ echo 1 > foo
  $ hg commit -m 'new 1'
  created new head
  $ echo 2 > foo
  $ hg commit -m 'new 2'
  $ hg -q up 2

Newer draft heads don't impact output

  $ hg show stack
    @  eaff draft 2
    o  2b21 draft 1
   /   (stack base)
  o  b66b base

Newer public heads are rendered

  $ hg phase --public -r '::tip'

  $ hg show stack
    o  baa4 new 2
   /    (2 commits ahead)
  :
  :    (stack head)
  : @  eaff draft 2
  : o  2b21 draft 1
  :/   (stack base)
  o  b66b base

If rebase is available, we show a hint how to rebase to that head

  $ hg --config extensions.rebase= show stack
    o  baa4 new 2
   /    (2 commits ahead; hg rebase --source 2b21 --dest baa4)
  :
  :    (stack head)
  : @  eaff draft 2
  : o  2b21 draft 1
  :/   (stack base)
  o  b66b base

Similar tests but for multiple heads

  $ hg -q up 0
  $ echo h2 > foo
  $ hg -q commit -m 'new head 2'
  $ hg phase --public -r .
  $ hg -q up 2

  $ hg show stack
    o  baa4 new 2
   /    (2 commits ahead)
  : o  9a84 new head 2
  :/    (1 commits ahead)
  :
  :    (stack head)
  : @  eaff draft 2
  : o  2b21 draft 1
  :/   (stack base)
  o  b66b base

  $ hg --config extensions.rebase= show stack
    o  baa4 new 2
   /    (2 commits ahead; hg rebase --source 2b21 --dest baa4)
  : o  9a84 new head 2
  :/    (1 commits ahead; hg rebase --source 2b21 --dest 9a84)
  :
  :    (stack head)
  : @  eaff draft 2
  : o  2b21 draft 1
  :/   (stack base)
  o  b66b base
