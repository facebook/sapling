  $ enable sparse
  $ newrepo
  $ hg sparse include src
  $ mkdir src
  $ touch src/x
  $ hg commit -m x -A src/x

The root directory ("") should not be ignored

  $ hg debugshell -c 'ui.write("%s\n" % str(repo.dirstate._ignore.visitdir("")))'
  True
