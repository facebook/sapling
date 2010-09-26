
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert=
  > [convert]
  > hg.saverev=False
  > EOF
  $ hg init orig
  $ cd orig
  $ echo foo > foo
  $ echo bar > bar
  $ hg ci -qAm 'add foo and bar'
  $ hg rm foo
  $ hg ci -m 'remove foo'
  $ mkdir foo
  $ echo file > foo/file
  $ hg ci -qAm 'add foo/file'
  $ hg tag some-tag
  $ hg log
  changeset:   3:593cbf6fb2b4
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag some-tag for changeset ad681a868e44
  
  changeset:   2:ad681a868e44
  tag:         some-tag
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo/file
  
  changeset:   1:cbba8ecc03b7
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     remove foo
  
  changeset:   0:327daa9251fa
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo and bar
  
  $ cd ..
  $ hg convert orig new 2>&1 | grep -v 'subversion python bindings could not be loaded'
  initializing destination new repository
  scanning source...
  sorting...
  converting...
  3 add foo and bar
  2 remove foo
  1 add foo/file
  0 Added tag some-tag for changeset ad681a868e44
  $ cd new
  $ hg out ../orig
  comparing with ../orig
  searching for changes
  no changes found
  [1]

dirstate should be empty:

  $ hg debugstate
  $ hg parents -q
  $ hg up -C
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg copy bar baz

put something in the dirstate:

  $ hg debugstate > debugstate
  $ grep baz debugstate
  a   0         -1 unset               baz
  copy: bar -> baz

add a new revision in the original repo

  $ cd ../orig
  $ echo baz > baz
  $ hg ci -qAm 'add baz'
  $ cd ..
  $ hg convert orig new 2>&1 | grep -v 'subversion python bindings could not be loaded'
  scanning source...
  sorting...
  converting...
  0 add baz
  $ cd new
  $ hg out ../orig
  comparing with ../orig
  searching for changes
  no changes found
  [1]

dirstate should be the same (no output below):

  $ hg debugstate > new-debugstate
  $ diff debugstate new-debugstate

no copies

  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugrename baz
  baz not renamed
  $ cd ..

test tag rewriting

  $ cat > filemap <<EOF
  > exclude foo
  > EOF
  $ hg convert --filemap filemap orig new-filemap 2>&1 | grep -v 'subversion python bindings could not be loaded'
  initializing destination new-filemap repository
  scanning source...
  sorting...
  converting...
  4 add foo and bar
  3 remove foo
  2 add foo/file
  1 Added tag some-tag for changeset ad681a868e44
  0 add baz
  $ cd new-filemap
  $ hg tags
  tip                                2:6f4fd1df87fb
  some-tag                           0:ba8636729451
  $ cd ..
