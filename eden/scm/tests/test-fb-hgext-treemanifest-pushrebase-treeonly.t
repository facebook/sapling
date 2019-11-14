Push treeonly commits from a treeonly shallow repo to a treeonly server

  $ setconfig remotefilelog.reponame=x remotefilelog.cachepath=$TESTTMP/cache ui.ssh="python $TESTDIR/dummyssh"
  $ setconfig treemanifest.flatcompat=False

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
  $ hg push --to foo
  pushing rev 0560779f58ae to destination ssh://user@dummy/server bookmark foo
  searching for changes
  remote: pushing 2 changesets:
  remote:     e297a1e684b7  C
  remote:     0560779f58ae  D
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  2 new obsolescence markers
  updating bookmark foo
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  obsoleted 2 changesets

  $ tglog --stat -l 2
  @  5: d9ee86e3acc1 'D'   sub/C |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  4: 4197fbd39b1b 'C'   sub/C |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  ~
