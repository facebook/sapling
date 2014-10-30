Tests that the various help files are properly registered

Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ hg help | grep 'git' | sed 's/  */ /g'
   hggit push and pull from a Git server
   git Working with Git Repositories
  $ hg help hggit | grep 'help git' | sed 's/:hg:`help git`/"hg help git"/g'
  For more information and instructions, see "hg help git"
  $ hg help git | grep 'Working with Git Repositories'
  Working with Git Repositories
