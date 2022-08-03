#chg-compatible

  $ setconfig workingcopy.ruststatus=False
Push treeonly commits from a treeonly shallow repo to a treeonly server

  $ setconfig remotefilelog.reponame=x remotefilelog.cachepath=$TESTTMP/cache
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False
  $ configure dummyssh

  $ newrepo server --config extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ setconfig treemanifest.server=True extensions.treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  $ enable pushrebase

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
  exporting bookmark foo
  remote: pushing 2 changesets:
  remote:     426bada5c675  A
  remote:     112478962961  B

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
  adding changesets
  adding manifests
  adding file changes
  updating bookmark foo
  remote: pushing 2 changesets:
  remote:     e297a1e684b7  C
  remote:     0560779f58ae  D
  remote: 2 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ tglog --stat -l 2
  @  d9ee86e3acc1 'D'  sub/C |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  4197fbd39b1b 'C'  sub/C |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  ~
