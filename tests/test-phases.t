  $ alias hglog='hg log --template "{rev} {phase} {desc}\n"'
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
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
