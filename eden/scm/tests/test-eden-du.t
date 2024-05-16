
#require eden

setup backing repo

  $ eagerepo
  $ newrepo backingrepo
  $ eden clone --allow-empty-repo $TESTTMP/backingrepo $TESTTMP/wcrepo
  Cloning new repository at $TESTTMP/wcrepo...
  Success.  Checked out commit 00000000

test eden du

  $ cd $TESTTMP/wcrepo
  $ seq 158730 > tmp.txt
  $ hg add tmp.txt
  $ hg commit -m "test commit"
  $ eden du | grep "Materialized files" | sed 's/\s+$/\n/' | sed 's/\s*//' # Windows has a nondeterministic split between materialized and backing repo
  Materialized files:  1.* MB (glob) (linux !)
  Materialized files:  1.* MB (glob) (osx !)
  Materialized files:  ***** KB (glob) (windows !)

test eden du - ignored files

  $ cd $TESTTMP/wcrepo
  $ echo "aaaaa" > tmp2.txt
  $ echo "tmp2.txt" > .gitignore
  $ eden du | grep "Ignored files:" | sed 's/\s+$/\n/' | sed 's/\s*//'
  Ignored files:  15 B
  $ rm tmp2.txt rm .gitignore

#if no-windows
test eden du --clean

  $ cd $TESTTMP/wcrepo
  $ seq 158730 > tmp.txt
  $ eden du --clean | grep "Materialized files:" | sed 's/\s+$/\n/' | sed 's/\s*//'
  Materialized files:  1.* MB    Not cleaned. Please see WARNING above (glob)
#endif
