  $ alias hglog='hg log --template "{rev} {phaseidx} {desc}\n"'
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    message="$1"
  >    shift
  >    hg ci -m "$message" $*
  > }

  $ hg init initialrepo
  $ cd initialrepo
  $ mkcommit A

New commit are draft by default

  $ hglog
  0 1 A

Following commit are draft too

  $ mkcommit B

  $ hglog
  1 1 B
  0 1 A

Draft commit are properly created over public one:

  $ hg phase --public .
  $ hglog
  1 0 B
  0 0 A

  $ mkcommit C
  $ mkcommit D

  $ hglog
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test creating changeset as secret

  $ mkcommit E --config phases.new-commit=2
  $ hglog
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test the secret property is inherited

  $ mkcommit H
  $ hglog
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Even on merge

  $ hg up -q 1
  $ mkcommit "B'"
  created new head
  $ hglog
  6 1 B'
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A
  $ hg merge 4 # E
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge B' and E"
  $ hglog
  7 2 merge B' and E
  6 1 B'
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A

Test secret changeset are not pushed

  $ hg init ../push-dest
  $ cat > ../push-dest/.hg/hgrc << EOF
  > [phases]
  > publish=False
  > EOF
  $ hg outgoing ../push-dest --template='{rev} {phase} {desc|firstline}\n'
  comparing with ../push-dest
  searching for changes
  0 public A
  1 public B
  2 draft C
  3 draft D
  6 draft B'
  $ hg outgoing -r default ../push-dest --template='{rev} {phase} {desc|firstline}\n'
  comparing with ../push-dest
  searching for changes
  0 public A
  1 public B
  2 draft C
  3 draft D
  6 draft B'

  $ hg push ../push-dest -f # force because we push multiple heads
  pushing to ../push-dest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 5 files (+1 heads)
  $ hglog
  7 2 merge B' and E
  6 1 B'
  5 2 H
  4 2 E
  3 1 D
  2 1 C
  1 0 B
  0 0 A
  $ cd ../push-dest
  $ hglog
  4 1 B'
  3 1 D
  2 1 C
  1 0 B
  0 0 A
  $ cd ..

Test secret changeset are not pull

  $ hg init pull-dest
  $ cd pull-dest
  $ hg pull ../initialrepo
  pulling from ../initialrepo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 5 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hglog
  4 0 B'
  3 0 D
  2 0 C
  1 0 B
  0 0 A
  $ cd ..

But secret can still be bundled explicitly

  $ cd initialrepo
  $ hg bundle --base '4^' -r 'children(4)' ../secret-bundle.hg
  4 changesets found
  $ cd ..

Test secret changeset are not cloned
(during local clone)

  $ hg clone -qU initialrepo clone-dest
  $ hglog -R clone-dest
  4 0 B'
  3 0 D
  2 0 C
  1 0 B
  0 0 A

Test revset

  $ cd initialrepo
  $ hglog -r 'public()'
  0 0 A
  1 0 B
  $ hglog -r 'draft()'
  2 1 C
  3 1 D
  6 1 B'
  $ hglog -r 'secret()'
  4 2 E
  5 2 H
  7 2 merge B' and E

Test phase command
===================

initial picture

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > hgext.graphlog=
  > EOF
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 secret E
  | |
  o |  3 draft D
  | |
  o |  2 draft C
  |/
  o  1 public B
  |
  o  0 public A
  

display changesets phase

(mixing -r and plain rev specification)

  $ hg phase 1::4 -r 7
  1: public
  2: draft
  3: draft
  4: secret
  7: secret


move changeset forward

(with -r option)

  $ hg phase --public -r 2
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 secret E
  | |
  o |  3 draft D
  | |
  o |  2 public C
  |/
  o  1 public B
  |
  o  0 public A
  

move changeset backward

(without -r option)

  $ hg phase --draft --force 2
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 secret E
  | |
  o |  3 draft D
  | |
  o |  2 draft C
  |/
  o  1 public B
  |
  o  0 public A
  

move changeset forward and backward

  $ hg phase --draft --force 1::4
  $ hg log -G --template "{rev} {phase} {desc}\n"
  @    7 secret merge B' and E
  |\
  | o  6 draft B'
  | |
  +---o  5 secret H
  | |
  o |  4 draft E
  | |
  o |  3 draft D
  | |
  o |  2 draft C
  |/
  o  1 draft B
  |
  o  0 public A
  
