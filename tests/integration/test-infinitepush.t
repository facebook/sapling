  $ . $TESTDIR/library.sh

setup configuration
  $ setup_common_config

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{node}\n'
  3903775176ed42b1458a6281db4a0ccf4d9f287a

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push and repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo


Do infinitepush (aka commit cloud) push
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > infinitepush=
  > infinitepushbackup=
  > [infinitepush]
  > server=False
  > EOF
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ hg addremove -q
  $ hg ci -m new
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --debug
  pushing to ssh://user@dummy/repo
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog pushkey bundle2=* (glob)
  remote: 1
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  1 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" 250 bytes payload
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 0 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes

  $ cd ../repo-pull
  $ hgmn pull -r 47da8b81097c5534f3eb7947a8764dd323cffe3d
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 47da8b81097c
  $ hgmn up -q 47da8b81097c
  $ cat newfile
  new

Pushbackup also works
  $ cd ../repo-push
  $ echo aa > aa && hg addremove && hg ci -q -m newrepo
  adding aa
  $ hgmn pushbackup ssh://user@dummy/repo --debug
  starting backup* (glob)
  backing up stack rooted at 47da8b81097c
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog pushkey bundle2=* (glob)
  remote: 1
  2 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  95cad53aab1b0b33eceee14473b3983312721529
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" 250 bytes payload
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 0 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 2 parts total
  bundle2-output-part: "replycaps" 250 bytes payload
  bundle2-output-part: "B2X:INFINITEPUSHSCRATCHBOOKMARKS" * bytes payload (glob)
  backup complete
  heads added: 95cad53aab1b0b33eceee14473b3983312721529
  heads removed: 
  finished in * seconds (glob)

  $ cd ../repo-pull
  $ hgmn pull -r 95cad53aab1b0b33eceee14473b3983312721529
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 95cad53aab1b
  $ hgmn up -q 95cad53aab1b0b33ecee
  $ cat aa
  aa

Pushbackup that pushes only bookmarks
  $ cd ../repo-push
  $ hg book newbook
  $ hgmn pushbackup ssh://user@dummy/repo --debug
  starting backup* (glob)
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog pushkey bundle2=* (glob)
  remote: 1
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 2 parts total
  bundle2-output-part: "replycaps" 250 bytes payload
  bundle2-output-part: "B2X:INFINITEPUSHSCRATCHBOOKMARKS" * bytes payload (glob)
  backup complete
  heads added: 
  heads removed: 
  finished in * seconds (glob)
