  $ "$TESTDIR/hghave" symlink execbit || exit 80

  $ tellmeabout() {
  > if [ -h $1 ]; then
  >     echo $1 is a symlink:
  >     $TESTDIR/readlink.py $1
  > elif [ -x $1 ]; then
  >     echo $1 is an executable file with content:
  >     cat $1
  > else
  >     echo $1 is a plain file with content:
  >     cat $1
  > fi
  > }

  $ hg init test1
  $ cd test1

  $ echo a > a
  $ hg ci -Aqmadd
  $ chmod +x a
  $ hg ci -mexecutable

  $ hg up -q 0
  $ rm a
  $ ln -s symlink a
  $ hg ci -msymlink
  created new head

Symlink is local parent, executable is other:

  $ hg merge --debug
    searching for copies back to rev 1
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: c334dc3be0da, local: 521a1e40188f+, remote: 3574f3e69b1c
   a: versions differ -> m
    preserving a for resolve of a
  updating: a 1/1 files (100.00%)
  picked tool 'internal:merge' for a (binary False symlink True)
  merging a
  my a@521a1e40188f+ other a@3574f3e69b1c ancestor a@c334dc3be0da
  warning: internal:merge cannot merge symlinks for a
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ tellmeabout a
  a is a symlink:
  a -> symlink
  $ hg resolve a --tool internal:other
  $ tellmeabout a
  a is an executable file with content:
  a
  $ hg st
  M a
  ? a.orig

Symlink is other parent, executable is local:

  $ hg update -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg merge --debug
    searching for copies back to rev 1
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: c334dc3be0da, local: 3574f3e69b1c+, remote: 521a1e40188f
   a: versions differ -> m
    preserving a for resolve of a
  updating: a 1/1 files (100.00%)
  picked tool 'internal:merge' for a (binary False symlink True)
  merging a
  my a@3574f3e69b1c+ other a@521a1e40188f ancestor a@c334dc3be0da
  warning: internal:merge cannot merge symlinks for a
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ tellmeabout a
  a is an executable file with content:
  a

Update to link without local change should get us a symlink (issue3316):

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st
  ? a.orig

Update to link with local change should cause a merge prompt (issue3200):

  $ hg up -Cq 0
  $ echo data > a
  $ HGMERGE= hg up -y --debug
    searching for copies back to rev 2
  resolving manifests
   branchmerge: False, force: False, partial: False
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


Test only 'l' change - happens rarely, except when recovering from situations
where that was what happened.

  $ hg init test2
  $ cd test2
  $ printf base > f
  $ hg ci -Aqm0
  $ echo file > f
  $ echo content >> f
  $ hg ci -qm1
  $ hg up -qr0
  $ rm f
  $ ln -s base f
  $ hg ci -qm2
  $ hg merge
  merging f
  warning: internal:merge cannot merge symlinks for f
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ tellmeabout f
  f is a symlink:
  f -> base

  $ hg up -Cqr1
  $ hg merge
  merging f
  warning: internal:merge cannot merge symlinks for f
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ tellmeabout f
  f is a plain file with content:
  file
  content

  $ cd ..

Test removed 'x' flag merged with change to symlink

  $ hg init test3
  $ cd test3
  $ echo f > f
  $ chmod +x f
  $ hg ci -Aqm0
  $ chmod -x f
  $ hg ci -qm1
  $ hg up -qr0
  $ rm f
  $ ln -s dangling f
  $ hg ci -qm2
  $ hg merge
  merging f
  warning: internal:merge cannot merge symlinks for f
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ tellmeabout f
  f is a symlink:
  f -> dangling

  $ hg up -Cqr1
  $ hg merge
  merging f
  warning: internal:merge cannot merge symlinks for f
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ tellmeabout f
  f is a plain file with content:
  f

Test removed 'x' flag merged with content change - both ways

  $ hg up -Cqr0
  $ echo change > f
  $ hg ci -qm3
  $ hg merge -r1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ tellmeabout f
  f is a plain file with content:
  change

  $ hg up -qCr1
  $ hg merge -r3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ tellmeabout f
  f is a plain file with content:
  change

  $ cd ..

Test merge with no common ancestor:
a: just different
b: x vs -, different (cannot calculate x, cannot ask merge tool)
c: x vs -, same (cannot calculate x, merge tool is no good)
d: x vs l, different
e: x vs l, same
f: - vs l, different
g: - vs l, same
h: l vs l, different
(where same means the filelog entry is shared and there thus is an ancestor!)

  $ hg init test4
  $ cd test4
  $ echo 0 > 0
  $ hg ci -Aqm0

  $ echo 1 > a
  $ echo 1 > b
  $ chmod +x b
  $ echo x > c
  $ chmod +x c
  $ echo 1 > d
  $ chmod +x d
  $ printf x > e
  $ chmod +x e
  $ echo 1 > f
  $ printf x > g
  $ ln -s 1 h
  $ hg ci -qAm1

  $ hg up -qr0
  $ echo 2 > a
  $ echo 2 > b
  $ echo x > c
  $ ln -s 2 d
  $ ln -s x e
  $ ln -s 2 f
  $ ln -s x g
  $ ln -s 2 h
  $ hg ci -Aqm2

  $ hg merge
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  warning: cannot merge flags for b
  merging b
  warning: conflicts during merge.
  merging b incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging d
  warning: internal:merge cannot merge symlinks for d
  merging d incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging f
  warning: internal:merge cannot merge symlinks for f
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging h
  warning: internal:merge cannot merge symlinks for h
  merging h incomplete! (edit conflicts, then use 'hg resolve --mark')
  3 files updated, 0 files merged, 0 files removed, 5 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg resolve -l
  U a
  U b
  U d
  U f
  U h
  $ tellmeabout a
  a is a plain file with content:
  <<<<<<< local
  2
  =======
  1
  >>>>>>> other
  $ tellmeabout b
  b is a plain file with content:
  <<<<<<< local
  2
  =======
  1
  >>>>>>> other
  $ tellmeabout c
  c is a plain file with content:
  x
  $ tellmeabout d
  d is a symlink:
  d -> 2
  $ tellmeabout e
  e is a symlink:
  e -> x
  $ tellmeabout f
  f is a symlink:
  f -> 2
  $ tellmeabout g
  g is a symlink:
  g -> x
  $ tellmeabout h
  h is a symlink:
  h -> 2

  $ hg up -Cqr1
  $ hg merge
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  warning: cannot merge flags for b
  merging b
  warning: conflicts during merge.
  merging b incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging d
  warning: internal:merge cannot merge symlinks for d
  merging d incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging f
  warning: internal:merge cannot merge symlinks for f
  merging f incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging h
  warning: internal:merge cannot merge symlinks for h
  merging h incomplete! (edit conflicts, then use 'hg resolve --mark')
  3 files updated, 0 files merged, 0 files removed, 5 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ tellmeabout a
  a is a plain file with content:
  <<<<<<< local
  1
  =======
  2
  >>>>>>> other
  $ tellmeabout b
  b is an executable file with content:
  <<<<<<< local
  1
  =======
  2
  >>>>>>> other
  $ tellmeabout c
  c is a plain file with content:
  x
  $ tellmeabout d
  d is an executable file with content:
  1
  $ tellmeabout e
  e is an executable file with content:
  x (no-eol)
  $ tellmeabout f
  f is a plain file with content:
  1
  $ tellmeabout g
  g is a plain file with content:
  x (no-eol)
  $ tellmeabout h
  h is a symlink:
  h -> 1

  $ cd ..
