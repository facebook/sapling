  $ setconfig extensions.treemanifest=!

Setup the test
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ enable infinitepush pushrebase
  $ cp "$HGRCPATH" "$TESTTMP/defaulthgrc"
  $ hg init repo1
  $ cd repo1
  $ setupserver
  $ cd ..
  $ hg init repo2
  $ cd repo2
  $ setupserver
  $ cd ..
  $ hg init repo3
  $ cd repo3
  $ setupserver
  $ cd ..

Check that we replicate a push
  $ hg clone ssh://user@dummy/repo1 client -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > infinitepush-other=ssh://user@dummy/repo2
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client
  $ mkcommit initialcommit
  $ hg push -r . --to scratch/test123 --create
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  please wait while we replicate this push to an alternate repository
  pushing to ssh://user@dummy/repo2
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ cd ..

Check that we do not replicate a push to the same destination
  $ hg clone ssh://user@dummy/repo1 client2 -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > infinitepush-other=ssh://user@dummy/repo1
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client2
  $ mkcommit initialcommit
  $ hg push -r . --to scratch/test123 --create
  pushing to ssh://user@dummy/repo1
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ cd ..
Check that we do not replicate a push when the destination is set
  $ hg clone ssh://user@dummy/repo1 client3 -q
  $ cp "$TESTTMP/defaulthgrc" "$HGRCPATH"
  $ cat >> "$HGRCPATH" << EOF
  > [paths]
  > infinitepush-other=ssh://user@dummy/repo2
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > EOF
  $ cd client3
  $ mkcommit initialcommit
  $ hg push ssh://user@dummy/repo3 -r . --to scratch/test123 --create
  pushing to ssh://user@dummy/repo3
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ cd ..
