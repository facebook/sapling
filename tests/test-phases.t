  $ alias hglog='hg log --template "{rev} {phase} {desc}\n"'

  $ hg init initialrepo
  $ cd initialrepo
  $ touch sam
  $ hg add sam
  $ hg ci -m 'first'

  $ hglog
  0 1 first
