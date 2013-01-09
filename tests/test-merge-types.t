  $ "$TESTDIR/hghave" symlink execbit || exit 80

  $ hg init

  $ echo a > a
  $ hg ci -Amadd
  adding a

  $ chmod +x a
  $ hg ci -mexecutable

  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm a
  $ ln -s symlink a
  $ hg ci -msymlink
  created new head

  $ hg merge --debug
    searching for copies back to rev 1
  resolving manifests
   overwrite: False, partial: False
   ancestor: c334dc3be0da, local: 521a1e40188f+, remote: 3574f3e69b1c
   conflicting flags for a
  (n)one, e(x)ec or sym(l)ink? n
   a: update permissions -> e
  updating: a 1/1 files (100.00%)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)


Symlink is local parent, executable is other:

  $ if [ -h a ]; then
  >     echo a is a symlink
  >     $TESTDIR/readlink.py a
  > elif [ -x a ]; then
  >     echo a is executable
  > else
  >     echo "a has no flags (default for conflicts)"
  > fi
  a has no flags (default for conflicts)

  $ hg update -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg merge --debug
    searching for copies back to rev 1
  resolving manifests
   overwrite: False, partial: False
   ancestor: c334dc3be0da, local: 3574f3e69b1c+, remote: 521a1e40188f
   conflicting flags for a
  (n)one, e(x)ec or sym(l)ink? n
   a: remote is newer -> g
  updating: a 1/1 files (100.00%)
  getting a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)


Symlink is other parent, executable is local:

  $ if [ -h a ]; then
  >    echo a is a symlink
  >    $TESTDIR/readlink.py a
  > elif [ -x a ]; then
  >     echo a is executable
  > else
  >     echo "a has no flags (default for conflicts)"
  > fi
  a has no flags (default for conflicts)

Update to link without local change should get us a symlink (issue3316):

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st

Update to link with local change should cause a merge prompt (issue3200):

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo data > a
  $ HGMERGE= hg up -y --debug
    searching for copies back to rev 2
  resolving manifests
   overwrite: False, partial: False
   ancestor: c334dc3be0da, local: c334dc3be0da+, remote: 521a1e40188f
   a: versions differ -> m
  preserving a for resolve of a
  updating: a 1/1 files (100.00%)
  (couldn't find merge tool hgmerge|tool hgmerge can't handle symlinks) (re)
  picked tool 'internal:prompt' for a (binary False symlink True)
   no tool found to merge a
  keep (l)ocal or take (o)ther? l
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg diff --git
  diff --git a/a b/a
  old mode 120000
  new mode 100644
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -symlink
  \ No newline at end of file
  +data


