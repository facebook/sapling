  $ echo "[extensions]" >> $HGRCPATH
  $ echo "purge=" >> $HGRCPATH
  $ echo "graphlog=" >> $HGRCPATH

  $ shortlog() {
  >     hg glog --template '{rev}:{node|short} {author} {date|hgdate} - {branch} - {desc|firstline}\n'
  > }

Test --bypass with other options

  $ hg init repo-options
  $ cd repo-options
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ hg branch foo
  marked working directory as branch foo
  $ hg ci -Am changea
  $ hg export . > ../test.diff
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test importing an existing revision

  $ hg import --bypass --exact ../test.diff
  applying ../test.diff
  $ shortlog
  o  1:4e322f7ce8e3 test 0 0 - foo - changea
  |
  o  0:07f494440405 test 0 0 - default - adda
  

Test failure without --exact

  $ hg import --bypass ../test.diff
  applying ../test.diff
  unable to find 'a' for patching
  abort: patch failed to apply
  [255]
  $ hg st
  $ shortlog
  o  1:4e322f7ce8e3 test 0 0 - foo - changea
  |
  o  0:07f494440405 test 0 0 - default - adda
  

Test --user, --date and --message

  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass --u test2 -d '1 0' -m patch2 ../test.diff
  applying ../test.diff
  $ cat .hg/last-message.txt
  patch2 (no-eol)
  $ shortlog
  o  2:2e127d1da504 test2 1 0 - default - patch2
  |
  | o  1:4e322f7ce8e3 test 0 0 - foo - changea
  |/
  @  0:07f494440405 test 0 0 - default - adda
  
  $ hg rollback
  repository tip rolled back to revision 1 (undo import)

Test --import-branch

  $ hg import --bypass --import-branch ../test.diff
  applying ../test.diff
  $ shortlog
  o  1:4e322f7ce8e3 test 0 0 - foo - changea
  |
  @  0:07f494440405 test 0 0 - default - adda
  
  $ hg rollback
  repository tip rolled back to revision 1 (undo import)

Test --strip

  $ hg import --bypass --strip 0 - <<EOF
  > # HG changeset patch
  > # User test
  > # Date 0 0
  > # Branch foo
  > # Node ID 4e322f7ce8e3e4203950eac9ece27bf7e45ffa6c
  > # Parent  07f4944404050f47db2e5c5071e0e84e7a27bba9
  > changea
  > 
  > diff -r 07f494440405 -r 4e322f7ce8e3 a
  > --- a	Thu Jan 01 00:00:00 1970 +0000
  > +++ a	Thu Jan 01 00:00:00 1970 +0000
  > @@ -1,1 +1,2 @@
  >  a
  > +a
  > EOF
  applying patch from stdin
  $ hg rollback
  repository tip rolled back to revision 1 (undo import)

Test unsupported combinations

  $ hg import --bypass --no-commit ../test.diff
  abort: cannot use --no-commit with --bypass
  [255]
  $ hg import --bypass --similarity 50 ../test.diff
  abort: cannot use --similarity with --bypass
  [255]

Test commit editor

  $ hg diff -c 1 > ../test.diff
  $ HGEDITOR=cat hg import --bypass ../test.diff
  applying ../test.diff
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed a
  abort: empty commit message
  [255]

Test patch.eol is handled

  $ python -c 'file("a", "wb").write("a\r\n")'
  $ hg ci -m makeacrlf
  $ hg import -m 'should fail because of eol' --bypass ../test.diff
  applying ../test.diff
  patching file a
  Hunk #1 FAILED at 0
  abort: patch failed to apply
  [255]
  $ hg --config patch.eol=auto import -d '0 0' -m 'test patch.eol' --bypass ../test.diff
  applying ../test.diff
  $ shortlog
  o  3:d7805b4d2cb3 test 0 0 - default - test patch.eol
  |
  @  2:872023de769d test 0 0 - default - makeacrlf
  |
  | o  1:4e322f7ce8e3 test 0 0 - foo - changea
  |/
  o  0:07f494440405 test 0 0 - default - adda
  

Test applying multiple patches

  $ hg up -qC 0
  $ echo e > e
  $ hg ci -Am adde
  adding e
  created new head
  $ hg export . > ../patch1.diff
  $ hg up -qC 1
  $ echo f > f
  $ hg ci -Am addf
  adding f
  $ hg export . > ../patch2.diff
  $ cd ..
  $ hg clone -r1 repo-options repo-multi1
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-multi1
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass ../patch1.diff ../patch2.diff
  applying ../patch1.diff
  applying ../patch2.diff
  $ shortlog
  o  3:bc8ca3f8a7c4 test 0 0 - default - addf
  |
  o  2:16581080145e test 0 0 - default - adde
  |
  | o  1:4e322f7ce8e3 test 0 0 - foo - changea
  |/
  @  0:07f494440405 test 0 0 - default - adda
  

Test applying multiple patches with --exact

  $ cd ..
  $ hg clone -r1 repo-options repo-multi2
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-multi2
  $ hg import --bypass --exact ../patch1.diff ../patch2.diff
  applying ../patch1.diff
  applying ../patch2.diff
  $ shortlog
  o  3:d60cb8989666 test 0 0 - foo - addf
  |
  | o  2:16581080145e test 0 0 - default - adde
  | |
  @ |  1:4e322f7ce8e3 test 0 0 - foo - changea
  |/
  o  0:07f494440405 test 0 0 - default - adda
  

  $ cd ..

Test complicated patch with --exact

  $ hg init repo-exact
  $ cd repo-exact
  $ echo a > a
  $ echo c > c
  $ echo d > d
  $ echo e > e
  $ echo f > f
  $ chmod +x f
  $ ln -s c linkc
  $ hg ci -Am t
  adding a
  adding c
  adding d
  adding e
  adding f
  adding linkc
  $ hg cp a aa1
  $ echo b >> a
  $ echo b > b
  $ hg add b
  $ hg cp a aa2
  $ echo aa >> aa2
  $ chmod +x e
  $ chmod -x f
  $ ln -s a linka
  $ hg rm d
  $ hg rm linkc
  $ hg mv c cc
  $ hg ci -m patch
  $ hg export --git . > ../test.diff
  $ hg up -C null
  0 files updated, 0 files merged, 7 files removed, 0 files unresolved
  $ hg purge
  $ hg st
  $ hg import --bypass --exact ../test.diff
  applying ../test.diff

The patch should have matched the exported revision and generated no additional
data. If not, diff both heads to debug it.

  $ shortlog
  o  1:2978fd5c8aa4 test 0 0 - default - patch
  |
  o  0:a0e19e636a43 test 0 0 - default - t
  
