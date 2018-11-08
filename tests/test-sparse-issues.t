  $ enable sparse
  $ newrepo
  $ hg sparse include a/b
  $ cat .hg/sparse
  [include]
  a/b
  [exclude]
  
  $ mkdir -p a/b b/c
  $ touch a/b/c b/c/d

BUG: b/c/d should not show up
  $ hg status
  ? a/b/c
  ? b/c/d

BUG: "<alwaysmatcher>" should not be used here
  $ hg dbsh -c 'print(repr(repo.dirstate._ignore))'
  <unionmatcher matchers=[<gitignorematcher>, <negatematcher matcher=<forceincludematcher matcher=<alwaysmatcher> includes=set([''])>>]>

More complex pattern
  $ hg sparse include 'a*/b*/c'
  $ mkdir -p a1/b1
  $ touch a1/b1/c
  $ hg status
  ? a/b/c
  ? a1/b1/c
  ? b/c/d
