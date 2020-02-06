# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ CACHEDIR=$PWD/cachepath
  $ . "${TEST_FIXTURES}/library.sh"

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
  $ hg cp test.txt copy.txt
  $ hg commit -Aqm "copy test.txt to test2.txt"
  $ COPY_FILENODE=$(hg manifest --debug | grep copy.txt | awk '{print $1}')
  $ TEST_ROOT_MANIFEST_NODE=$(hg log -r . -T '{manifest}')
  $ echo "line 2" >> test.txt
  $ echo "line 2" >> copy.txt
  $ hg commit -qm "add line 2 to test files"
  $ echo "line 3" >> test.txt
  $ echo "line 3" >> test2.txt
  $ hg commit -qm "add line 3 to test files"
  $ TEST_FILENODE2=$(hg manifest --debug | grep test.txt | awk '{print $1}')
  $ COPY_FILENODE2=$(hg manifest --debug | grep copy.txt | awk '{print $1}')

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
  > [edenapi]
  > enabled = true
  > url = $APISERVER
  > EOF

Check that the API server is alive
  $ hg debughttp > output
  $ diff output - <<< "successfully connected to: $HOSTNAME"

Test fetching single file
  $ DATAPACK_PATH=$(hg debuggetfile <<EOF | tail -n 1 | awk '{print $3}'
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
  $ DATAPACK_PATH=$(hg debuggetfile <<EOF | tail -n 1 | awk '{print $3}'
  > $TEST_FILENODE test.txt
  > $COPY_FILENODE copy.txt
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

Test fetching history for single file
  $ HISTPACK_PATH=$(hg debuggethistory <<EOF | tail -n 1 | awk '{print $3}'
  > $TEST_FILENODE2 test.txt
  > EOF
  > )

Verify that historypack has expected content
  $ hg debughistorypack $HISTPACK_PATH
  
  test.txt
  Node          P1 Node       P2 Node       Link Node     Copy From
  596c909aab72  b6fe30270546  000000000000  4af0b091e704  
  b6fe30270546  186cafa3319c  000000000000  6f445033ece9  
  186cafa3319c  000000000000  000000000000  f91e155a86e1  

Test fetching history for multiple files
  $ HISTPACK_PATH=$(hg debuggethistory <<EOF | tail -n 1 | awk '{print $3}'
  > $TEST_FILENODE2 test.txt
  > $COPY_FILENODE2 copy.txt
  > EOF
  > )

Verify that historypack has expected content
  $ hg debughistorypack $HISTPACK_PATH
  
  copy.txt
  Node          P1 Node       P2 Node       Link Node     Copy From
  672343a6daad  17b8d4e3bafd  000000000000  6f445033ece9  
  17b8d4e3bafd  186cafa3319c  000000000000  507881746c0f  test.txt
  
  test.txt
  Node          P1 Node       P2 Node       Link Node     Copy From
  596c909aab72  b6fe30270546  000000000000  4af0b091e704  
  b6fe30270546  186cafa3319c  000000000000  6f445033ece9  
  186cafa3319c  000000000000  000000000000  f91e155a86e1  

Test fetching only most recent history entry
  $ HISTPACK_PATH=$(hg debuggethistory --depth 1 <<EOF | tail -n 1 | awk '{print $3}'
  > $TEST_FILENODE2 test.txt
  > $COPY_FILENODE2 copy.txt
  > EOF
  > )
  $ hg debughistorypack $HISTPACK_PATH
  
  copy.txt
  Node          P1 Node       P2 Node       Link Node     Copy From
  672343a6daad  17b8d4e3bafd  000000000000  6f445033ece9  
  
  test.txt
  Node          P1 Node       P2 Node       Link Node     Copy From
  596c909aab72  b6fe30270546  000000000000  4af0b091e704  

Test fetching a single tree
  $ DATAPACK_PATH=$(hg debuggettrees <<EOF | tail -n 1 | awk '{print $3}'
  > $TEST_ROOT_MANIFEST_NODE
  > EOF
  > )

Verify that datapack has entry with expected content
  $ hg debugdatapack $DATAPACK_PATH
  $TESTTMP/cachepath/repo/packs/manifests/86dd528d5618ed64aae3c301efc771a09575b7e5:
  (empty name):
  Node          Delta Base    Delta Length  Blob Size
  c8743b14e078  000000000000  100           100
  
  Total:                      100           100       (0.0% bigger)
  $ hg debugdatapack $DATAPACK_PATH --node c8743b14e0789cc546125213c18a18d813862db5
  $TESTTMP/cachepath/repo/packs/manifests/86dd528d5618ed64aae3c301efc771a09575b7e5:
  copy.txt\x0017b8d4e3bafd4ec4812ad7c930aace9bf07ab033 (esc)
  test.txt\x00186cafa3319c24956783383dc44c5cbc68c5a0ca (esc)
