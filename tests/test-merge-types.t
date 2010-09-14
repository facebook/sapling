  $ hg init

  $ echo a > a
  $ hg ci -Amadd
  adding a

  $ chmod +x a
  $ hg ci -mexecutable

  $ hg up 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm a
  $ ln -s symlink a
  $ hg ci -msymlink
  created new head

  $ hg merge --debug
    searching for copies back to rev 1
  resolving manifests
   overwrite None partial False
   ancestor c334dc3be0da local 521a1e40188f+ remote 3574f3e69b1c
   conflicting flags for a
  (n)one, e(x)ec or sym(l)ink? n
   a: update permissions -> e
  updating: a 1/1 files (100.00%)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
   overwrite None partial False
   ancestor c334dc3be0da local 3574f3e69b1c+ remote 521a1e40188f
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

