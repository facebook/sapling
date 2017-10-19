test sparse

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > strip=
  > EOF

  $ echo a > show
  $ echo x > hide
  $ hg ci -Aqm 'initial'

  $ echo b > show
  $ echo y > hide
  $ echo aa > show2
  $ echo xx > hide2
  $ hg ci -Aqm 'two'

Verify basic --include

  $ hg up -q 0
  $ hg debugsparse --include 'hide'
  $ ls
  hide

Absolute paths outside the repo should just be rejected

#if no-windows
  $ hg debugsparse --include /foo/bar
  abort: paths cannot be absolute
  [255]
  $ hg debugsparse --include '$TESTTMP/myrepo/hide'

  $ hg debugsparse --include '/root'
  abort: paths cannot be absolute
  [255]
#else
TODO: See if this can be made to fail the same way as on Unix
  $ hg debugsparse --include /c/foo/bar
  abort: paths cannot be absolute
  [255]
  $ hg debugsparse --include '$TESTTMP/myrepo/hide'

  $ hg debugsparse --include '/c/root'
  abort: paths cannot be absolute
  [255]
#endif

Paths should be treated as cwd-relative, not repo-root-relative
  $ mkdir subdir && cd subdir
  $ hg debugsparse --include path
  $ hg debugsparse
  [include]
  $TESTTMP/myrepo/hide
  hide
  subdir/path
  
  $ cd ..
  $ echo hello > subdir/file2.ext
  $ cd subdir
  $ hg debugsparse --include '**.ext'  # let us test globs
  $ hg debugsparse --include 'path:abspath'  # and a path: pattern
  $ cd ..
  $ hg debugsparse
  [include]
  $TESTTMP/myrepo/hide
  hide
  path:abspath
  subdir/**.ext
  subdir/path
  
  $ rm -rf subdir

Verify commiting while sparse includes other files

  $ echo z > hide
  $ hg ci -Aqm 'edit hide'
  $ ls
  hide
  $ hg manifest
  hide
  show

Verify --reset brings files back

  $ hg debugsparse --reset
  $ ls
  hide
  show
  $ cat hide
  z
  $ cat show
  a

Verify 'hg debugsparse' default output

  $ hg up -q null
  $ hg debugsparse --include 'show*'

  $ hg debugsparse
  [include]
  show*
  
Verify update only writes included files

  $ hg up -q 0
  $ ls
  show

  $ hg up -q 1
  $ ls
  show
  show2

Verify status only shows included files

  $ touch hide
  $ touch hide3
  $ echo c > show
  $ hg status
  M show

Adding an excluded file should fail

  $ hg add hide3
  abort: cannot add 'hide3' - it is outside the sparse checkout
  (include file with `hg debugsparse --include <pattern>` or use `hg add -s <file>` to include file directory while adding)
  [255]

Verify deleting sparseness while a file has changes fails

  $ hg debugsparse --delete 'show*'
  pending changes to 'hide'
  abort: cannot change sparseness due to pending changes (delete the files or use --force to bring them back dirty)
  [255]

Verify deleting sparseness with --force brings back files

  $ hg debugsparse --delete -f 'show*'
  pending changes to 'hide'
  $ ls
  hide
  hide2
  hide3
  show
  show2
  $ hg st
  M hide
  M show
  ? hide3

Verify editing sparseness fails if pending changes

  $ hg debugsparse --include 'show*'
  pending changes to 'hide'
  abort: could not update sparseness due to pending changes
  [255]

Verify adding sparseness hides files

  $ hg debugsparse --exclude -f 'hide*'
  pending changes to 'hide'
  $ ls
  hide
  hide3
  show
  show2
  $ hg st
  M show

  $ hg up -qC .
TODO: add an option to purge to also purge files outside the sparse config?
  $ hg purge --all --config extensions.purge=
  $ ls
  hide
  hide3
  show
  show2
For now, manually remove the files
  $ rm hide hide3

Verify rebase temporarily includes excluded files

  $ hg rebase -d 1 -r 2 --config extensions.rebase=
  rebasing 2:b91df4f39e75 "edit hide" (tip)
  temporarily included 1 file(s) in the sparse checkout for merging
  merging hide
  warning: conflicts while merging hide! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg debugsparse
  [exclude]
  hide*
  
  Temporarily Included Files (for merge/rebase):
  hide

  $ cat hide
  <<<<<<< dest:   39278f7c08a9 - test: two
  y
  =======
  z
  >>>>>>> source: b91df4f39e75 - test: edit hide

Verify aborting a rebase cleans up temporary files

  $ hg rebase --abort --config extensions.rebase=
  cleaned up 1 temporarily added file(s) from the sparse checkout
  rebase aborted
  $ rm hide.orig

  $ ls
  show
  show2

Verify merge fails if merging excluded files

  $ hg up -q 1
  $ hg merge -r 2
  temporarily included 1 file(s) in the sparse checkout for merging
  merging hide
  warning: conflicts while merging hide! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg debugsparse
  [exclude]
  hide*
  
  Temporarily Included Files (for merge/rebase):
  hide

  $ hg up -C .
  cleaned up 1 temporarily added file(s) from the sparse checkout
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugsparse
  [exclude]
  hide*
  

Verify strip -k resets dirstate correctly

  $ hg status
  $ hg debugsparse
  [exclude]
  hide*
  
  $ hg log -r . -T '{rev}\n' --stat
  1
   hide  |  2 +-
   hide2 |  1 +
   show  |  2 +-
   show2 |  1 +
   4 files changed, 4 insertions(+), 2 deletions(-)
  
  $ hg strip -r . -k
  saved backup bundle to $TESTTMP/myrepo/.hg/strip-backup/39278f7c08a9-ce59e002-backup.hg (glob)
  $ hg status
  M show
  ? show2

Verify rebase succeeds if all changed files are in sparse checkout

  $ hg commit -Aqm "add show2"
  $ hg rebase -d 1 --config extensions.rebase=
  rebasing 2:bdde55290160 "add show2" (tip)
  saved backup bundle to $TESTTMP/myrepo/.hg/strip-backup/bdde55290160-216ed9c6-rebase.hg (glob)

Verify log --sparse only shows commits that affect the sparse checkout

  $ hg log -T '{rev} '
  2 1 0  (no-eol)
  $ hg log --sparse -T '{rev} '
  2 0  (no-eol)

Test status on a file in a subdir

  $ mkdir -p dir1/dir2
  $ touch dir1/dir2/file
  $ hg debugsparse -I dir1/dir2
  $ hg status
  ? dir1/dir2/file

Test that add -s adds dirs to sparse profile

  $ hg debugsparse --reset
  $ hg debugsparse --include empty
  $ hg debugsparse
  [include]
  empty
  

  $ mkdir add
  $ touch add/foo
  $ touch add/bar
  $ hg add add/foo
  abort: cannot add 'add/foo' - it is outside the sparse checkout
  (include file with `hg debugsparse --include <pattern>` or use `hg add -s <file>` to include file directory while adding)
  [255]
  $ hg add -s add/foo
  $ hg st
  A add/foo
  ? add/bar
  $ hg debugsparse
  [include]
  add
  empty
  
  $ hg add -s add/*
  add/foo already tracked!
  $ hg st
  A add/bar
  A add/foo
  $ hg debugsparse
  [include]
  add
  empty
  

  $ cd ..

Test non-sparse repos work while sparse is loaded
  $ hg init sparserepo
  $ hg init nonsparserepo
  $ cd sparserepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=
  > EOF
  $ cd ../nonsparserepo
  $ echo x > x && hg add x && hg commit -qAm x
  $ cd ../sparserepo
  $ hg clone ../nonsparserepo ../nonsparserepo2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test debugrebuilddirstate
  $ cd ../sparserepo
  $ touch included
  $ touch excluded
  $ hg add included excluded
  $ hg commit -m 'a commit' -q
  $ cp .hg/dirstate ../dirstateboth
  $ hg debugsparse -X excluded
  $ cp ../dirstateboth .hg/dirstate
  $ hg debugrebuilddirstate
  $ hg debugdirstate
  n   0         -1 unset               included

Test debugdirstate --minimal where file is in the parent manifest but not the
dirstate
  $ hg debugsparse -X included
  $ hg debugdirstate
  $ cp .hg/dirstate ../dirstateallexcluded
  $ hg debugsparse --reset
  $ hg debugsparse -X excluded
  $ cp ../dirstateallexcluded .hg/dirstate
  $ touch includedadded
  $ hg add includedadded
  $ hg debugdirstate --nodates
  a   0         -1 unset               includedadded
  $ hg debugrebuilddirstate --minimal
  $ hg debugdirstate --nodates
  n   0         -1 unset               included
  a   0         -1 * includedadded (glob)

Test debugdirstate --minimal where a file is not in parent manifest
but in the dirstate. This should take into account excluded files in the
manifest
  $ cp ../dirstateboth .hg/dirstate
  $ touch includedadded
  $ hg add includedadded
  $ touch excludednomanifest
  $ hg add excludednomanifest
  $ cp .hg/dirstate ../moreexcluded
  $ hg forget excludednomanifest
  $ rm excludednomanifest
  $ hg debugsparse -X excludednomanifest
  $ cp ../moreexcluded .hg/dirstate
  $ hg manifest
  excluded
  included
We have files in the dirstate that are included and excluded. Some are in the
manifest and some are not.
  $ hg debugdirstate --nodates
  n 644          0 * excluded (glob)
  a   0         -1 * excludednomanifest (glob)
  n 644          0 * included (glob)
  a   0         -1 * includedadded (glob)
  $ hg debugrebuilddirstate --minimal
  $ hg debugdirstate --nodates
  n 644          0 * included (glob)
  a   0         -1 * includedadded (glob)

