test sparse

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$(dirname $TESTDIR)/sparse.py
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
  $ hg sparse --include 'hide'
  $ ls
  hide

Verify commiting while sparse includes other files

  $ echo z > hide
  $ hg ci -Aqm 'edit hide'
  $ ls
  hide
  $ hg manifest
  hide
  show

Verify --reset brings files back

  $ hg sparse --reset
  $ ls
  hide
  show
  $ cat hide
  z
  $ cat show
  a

Verify 'hg sparse' default output

  $ hg up -q null
  $ hg sparse --include 'show*'

  $ hg sparse
  [include]
  show*
  [exclude]
  
  

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
  [255]

Verify deleting sparseness while a file has changes fails

  $ hg sparse --delete 'show*'
  pending changes to 'hide'
  abort: cannot change sparseness due to pending changes (delete the files or use --force to bring them back dirty)
  [255]

Verify deleting sparseness with --force brings back files

  $ hg sparse --delete -f 'show*'
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

Verify editting sparseness fails if pending changes

  $ hg sparse --include 'show*'
  pending changes to 'hide'
  abort: could not update sparseness due to pending changes
  [255]

Verify adding sparseness hides files

  $ hg sparse --exclude -f 'hide*'
  pending changes to 'hide'
  $ ls
  hide
  hide3
  show
  show2
  $ hg st
  M show

  $ hg up -qC .
  $ hg purge --all --config extensions.purge=
  $ ls
  show
  show2

Verify rebase fails if moving excluded files

  $ hg rebase -d 1 -r 2 --config extensions.rebase=
  abort: cannot merge because hide is outside the sparse checkout
  [255]

  $ hg rebase --abort --config extensions.rebase=
  rebase aborted

Verify merge fails if merging excluded files

  $ hg up -q 1
  $ hg merge -r 2
  abort: cannot merge because hide is outside the sparse checkout
  [255]
  $ hg up -qC .

Verify strip -k resets dirstate correctly

  $ hg status
  $ hg sparse
  [include]
  
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
  saved backup bundle to $TESTTMP/myrepo/.hg/strip-backup/39278f7c08a9-backup.hg
  $ hg status
  M show
  ? show2
