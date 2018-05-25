#testcases with-remotenames without-remotenames

#if with-remotenames
  $ enable remotenames
#endif

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ hg clone -q . $TESTTMP/cloned
  $ cd $TESTTMP/cloned
  $ drawdag << 'EOS'
  > C
  > |
  > A
  > EOS
  $ hg up tip -q


#if with-remotenames

Remotenames adds extra checks for bookmarks. It requires "--allow-anon":

  $ hg push
  pushing to $TESTTMP/repo1
  searching for changes
  abort: push would create new anonymous heads (dc0947a82db8)
  (use --allow-anon to override this warning)
  [255]

However, push still fails with "--allow-anon", because of the checkheads:

  $ hg push --allow-anon
  pushing to $TESTTMP/repo1
  searching for changes
  abort: push creates new remote head dc0947a82db8!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]

The checkheads feature should probably be disabled automatically if remotenames
is enabled.

Push with checkheads disabled:

  $ hg push --config ui.checkheads=0 --allow-anon
  pushing to $TESTTMP/repo1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)

#else

To be compatible with the legacy behavior, pushing a new head is forbidden:

  $ hg push
  pushing to $TESTTMP/repo1
  searching for changes
  abort: push creates new remote head dc0947a82db8!
  (merge or see 'hg help push' for details about pushing new heads)
  [255]

The check could be disabled:

  $ hg push --config ui.checkheads=0
  pushing to $TESTTMP/repo1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
#endif

