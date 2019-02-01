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
  $ hg commit -Aqm "add test.txt"
  $ TEST_FILENODE=$(hg manifest --debug | grep test.txt | cut -d ' ' -f 1)
  $ hg cp test.txt test2.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COPY_FILENODE=$(hg manifest --debug | grep test2.txt | cut -d ' ' -f 1)
  $ hg bookmarks -r tip master_bookmark

Blobimport test repo
  $ cd ..
  $ blobimport repo-hg/.hg repo

Start API server
  $ APISERVER_PORT=$(get_free_socket)
  $ no_ssl_apiserver -H "127.0.0.1" -p $APISERVER_PORT
  $ wait_for_apiserver --no-ssl

Enable Mononoke API for Mercurial client
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-repo
  $ cd client-repo
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > reponame = repo
  > [mononoke-api]
  > enabled = true
  > host = $APISERVER
  > EOF

Check that the API server is alive.
  $ hg debughttphealthcheck
  successfully connected to: http://localhost:* (glob)

Test fetching file contents
  $ hg debuggetfile <<EOF
  > $TEST_FILENODE test.txt
  > $COPY_FILENODE test2.txt
  > EOF
  wrote datapack: $TESTTMP/cachepath/repo/packs/05a5c5f239f359ccf6200acfa30af03cf77b174f

Verify contents
  $ hg debugdatapack $TESTTMP/cachepath/repo/packs/05a5c5f239f359ccf6200acfa30af03cf77b174f --node $TEST_FILENODE
  $TESTTMP/cachepath/repo/packs/05a5c5f239f359ccf6200acfa30af03cf77b174f:
  test content

  $ hg debugdatapack $TESTTMP/cachepath/repo/packs/05a5c5f239f359ccf6200acfa30af03cf77b174f --node $COPY_FILENODE
  $TESTTMP/cachepath/repo/packs/05a5c5f239f359ccf6200acfa30af03cf77b174f:
  \x01 (esc)
  copy: test.txt
  copyrev: 186cafa3319c24956783383dc44c5cbc68c5a0ca
  \x01 (esc)
  test content
