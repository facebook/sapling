Tests that the various help files are properly registered

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=$(echo $(dirname $TESTDIR))/hggit" >> $HGRCPATH

  $ hg help | grep 'git' | sed 's/  */ /g'
   hggit push and pull from a Git server
   git Working with Git Repositories
  $ hg help hggit | grep 'help git' | sed 's/:hg:`help git`/"hg help git"/g'
  For more information and instructions, see "hg help git"
  $ hg help git | grep 'Working with Git Repositories'
  Working with Git Repositories
