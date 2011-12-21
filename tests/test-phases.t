  $ alias hglog='hg log --template "{rev} {phase} {desc}\n"'
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

  $ hg pull -q . # XXX use the dedicated phase command once available
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
  $ hg push ../push-dest -f # force because we push multiple heads
  pushing to ../push-dest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 5 files (+1 heads)
  $ hglog
  7 2 merge B' and E
  6 0 B'
  5 2 H
  4 2 E
  3 0 D
  2 0 C
  1 0 B
  0 0 A
  $ cd ../push-dest
  $ hglog
  4 0 B'
  3 0 D
  2 0 C
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
