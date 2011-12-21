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

