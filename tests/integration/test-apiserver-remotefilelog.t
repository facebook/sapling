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
  $ TEST_FILENODE=$(hg manifest --debug | grep test.txt | awk '{print $1}')
  $ hg cp test.txt test2.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COPY_FILENODE=$(hg manifest --debug | grep test2.txt | awk '{print $1}')
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
  > url = $APISERVER
  > EOF

Check that the API server is alive
  $ hg debughttphealthcheck
  successfully connected to: http://localhost:* (glob)

Test fetching single file
  $ DATAPACK_PATH=$(hg debuggetfile <<EOF | awk '{print $3}'
  > $TEST_FILENODE test.txt
  > EOF
  > )

Verify that datapack has entry with expected metadata
  $ hg debugdatapack $DATAPACK_PATH
  $TESTTMP/cachepath/repo/packs/*: (glob)
  test.txt:
  Node          Delta Base    Delta Length  Blob Size
  186cafa3319c  000000000000  13            13
  
  Total:                      13            13        (0.0% bigger)

Test fetching multiple files
  $ DATAPACK_PATH=$(hg debuggetfile <<EOF | awk '{print $3}'
  > $TEST_FILENODE test.txt
  > $COPY_FILENODE test2.txt
  > EOF
  > )

Verify file contents
  $ hg debugdatapack $DATAPACK_PATH --node $TEST_FILENODE
  $TESTTMP/cachepath/repo/packs/*: (glob)
  test content

  $ hg debugdatapack $DATAPACK_PATH --node $COPY_FILENODE
  $TESTTMP/cachepath/repo/packs/*: (glob)
  \x01 (esc)
  copy: test.txt
  copyrev: 186cafa3319c24956783383dc44c5cbc68c5a0ca
  \x01 (esc)
  test content
