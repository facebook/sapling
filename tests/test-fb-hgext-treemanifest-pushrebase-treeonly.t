Push treeonly commits from a treeonly shallow repo to a treeonly server

  $ setconfig remotefilelog.reponame=x remotefilelog.cachepath=$TESTTMP/cache ui.ssh="python $TESTDIR/dummyssh"

  $ newrepo server
  $ setconfig treemanifest.server=True
  $ enable pushrebase treemanifest

  $ newrepo client
  $ setconfig paths.default=ssh://user@dummy/server
  $ echo remotefilelog >> .hg/requires
  $ enable treemanifest remotefilelog pushrebase remotenames
  $ setconfig treemanifest.sendtrees=True treemanifest.treeonly=True
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS

  $ hg push --to foo -r $B --create
  pushing rev 112478962961 to destination ssh://user@dummy/server bookmark foo
  searching for changes
  remote: pushing 2 changesets:
  remote:     426bada5c675  A
  remote:     112478962961  B
  exporting bookmark foo

Make server treeonly and push trees to it
  $ switchrepo server
  $ setconfig treemanifest.treeonly=True

  $ switchrepo client
  $ hg up $A
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir sub
  $ echo >> sub/C
  $ hg commit -Aqm "C"
  $ echo >> sub/C
  $ hg commit -qm "D"

# BUG: This should succeed
  $ hg push --to foo > /dev/null 2>&1
  [255]

  $ tglog --stat -l 2
  @  3: 0560779f58ae 'D'   sub/C |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  2: e297a1e684b7 'C'   sub/C |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  ~
