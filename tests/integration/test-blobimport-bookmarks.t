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

  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
  $ mononoke_admin bookmarks list --kind publishing
  * using repo "repo" repoid RepositoryId(0) (glob)
  master	* (glob)
  $ rm -rf repo

  $ blobimport repo-hg/.hg repo --no-bookmark
  $ mononoke_admin bookmarks list --kind publishing
  * using repo "repo" repoid RepositoryId(0) (glob)
  $ rm -rf repo

  $ blobimport repo-hg/.hg repo --prefix-bookmark myrepo/
  $ mononoke_admin bookmarks list --kind publishing
  * using repo "repo" repoid RepositoryId(0) (glob)
  myrepo/master	* (glob)
