  $ . "${TEST_FIXTURES}/library.sh"

# setup repo, usefncache flag for forcing algo encoding run
  $ hg init repo-hg --config format.usefncache=False

# Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF
  $ echo hello > world
  $ hg commit -Aqm "some commit"
  $ hg bookmark -r . master

  $ REPOID=0 setup_mononoke_config
  $ cd $TESTTMP
  $ REPOID=0 blobimport repo-hg/.hg repo
  $ REPOID=0 mononoke_admin bookmarks list --kind publishing
  * using repo "repo" repoid RepositoryId(0) (glob)
  master	* (glob)
  $ rm -rf repo

  $ REPOID=1 setup_mononoke_config
  $ cd $TESTTMP
  $ REPOID=1 blobimport repo-hg/.hg repo --no-bookmark
  $ REPOID=1 mononoke_admin bookmarks list --kind publishing
  * using repo "repo" repoid RepositoryId(1) (glob)
  $ rm -rf repo

  $ REPOID=2 setup_mononoke_config
  $ cd $TESTTMP
  $ REPOID=2 blobimport repo-hg/.hg repo --prefix-bookmark myrepo/
  $ REPOID=2 mononoke_admin bookmarks list --kind publishing
  * using repo "repo" repoid RepositoryId(2) (glob)
  myrepo/master	* (glob)
