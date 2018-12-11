  $ CACHEDIR=$PWD/cachepath
  $ . $TESTDIR/library.sh

Set up local hgrc and Mononoke config repo
  $ setup_common_config
  $ cd $TESTTMP

Initialize test repo
  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ setup_hg_server

Populate test repo
  $ echo "test content" > test.txt
  $ hg commit -Aqm "test commit"
  $ hg bookmarks -r tip master_bookmark

Blobimport test repo
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start API server
  $ APISERVER_PORT=$(get_free_socket)
  $ no_ssl_apiserver -H "127.0.0.1" -p $APISERVER_PORT
  $ wait_for_apiserver --no-ssl

Enable Mononoke API for Mercurial client
  $ cd repo-hg
  $ cat >> $HGRCPATH <<EOF
  > [mononoke-api]
  > enabled = true
  > host = $APISERVER
  > EOF

  $ hg debugmononokeapi
  success
