  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > mq=
  > [alias]
  > tlog = log --template "{rev}: {desc}\\n"
  > theads = heads --template "{rev}: {desc}\\n"
  > tincoming = incoming --template "{rev}: {desc}\\n"
  > EOF

Setup main:

  $ hg init base
  $ cd base
  $ echo "One" > one
  $ hg add
  adding one
  $ hg ci -m "main: one added"
  $ echo "++" >> one
  $ hg ci -m "main: one updated"

Bundle main:

  $ hg bundle --base=null ../main.hg
  2 changesets found

  $ cd ..

Incoming to fresh repo:

  $ hg init fresh

  $ hg -R fresh tincoming main.hg
  comparing with main.hg
  0: main: one added
  1: main: one updated
  $ test -f ./fresh/.hg/hg-bundle* && echo 'temp. bundle file remained' || true

  $ hg -R fresh tincoming bundle:fresh+main.hg
  comparing with bundle:fresh+main.hg
  0: main: one added
  1: main: one updated


Setup queue:

  $ cd base
  $ hg qinit -c
  $ hg qnew -m "patch: two added" two.patch
  $ echo two > two
  $ hg add
  adding two
  $ hg qrefresh
  $ hg qcommit -m "queue: two.patch added"
  $ hg qpop -a
  popping two.patch
  patch queue now empty

Bundle queue:

  $ hg -R .hg/patches bundle --base=null ../queue.hgq
  1 changesets found
  $ test -f ./fresh/.hg/hg-bundle* && echo 'temp. bundle file remained' || true

  $ cd ..


Clone base:

  $ hg clone base copy
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd copy
  $ hg qinit -c

Incoming queue bundle:

  $ hg -R .hg/patches tincoming ../queue.hgq
  comparing with ../queue.hgq
  0: queue: two.patch added
  $ test -f .hg/hg-bundle* && echo 'temp. bundle file remained' || true

Pull queue bundle:

  $ hg -R .hg/patches pull --update ../queue.hgq
  pulling from ../queue.hgq
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  merging series
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ test -f .hg/patches/hg-bundle* && echo 'temp. bundle file remained' || true

  $ hg -R .hg/patches theads
  0: queue: two.patch added

  $ hg -R .hg/patches tlog
  0: queue: two.patch added

  $ hg qseries
  two.patch

  $ cd ..


Clone base again:

  $ hg clone base copy2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd copy2
  $ hg qinit -c

Unbundle queue bundle:

  $ hg -R .hg/patches unbundle --update ../queue.hgq
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  merging series
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved

  $ hg -R .hg/patches theads
  0: queue: two.patch added

  $ hg -R .hg/patches tlog
  0: queue: two.patch added

  $ hg qseries
  two.patch

  $ cd ..

