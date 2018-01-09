Tests that the various help files are properly registered

Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ hg help | grep 'git'
   hggit         push and pull from a Git server
   git           Working with Git Repositories

Mercurial 3.7+ uses single quotes
  $ hg help hggit | grep 'help git'
  For more information and instructions, see 'hg help git'
  $ hg help git | grep 'Working with Git Repositories'
  Working with Git Repositories
