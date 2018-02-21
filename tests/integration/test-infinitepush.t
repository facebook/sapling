  $ . $TESTDIR/library.sh

setup configuration
  $ setup_config_repo
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > remotefilelog=
  > [remotefilelog]
  > cachepath=$TESTTMP/cachepath
  > EOF


setup repo

  $ hg init repo-hg

Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

  $ touch a
  $ hg add a
  $ hg ci -ma
  $ hg log
  changeset:   0:3903775176ed
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ cd $TESTTMP

setup repo2
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate

  $ blobimport --blobstore files --linknodes repo-hg repo

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ wait_for_mononoke $TESTTMP/repo

  $ cd repo2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > infinitepush=
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
  remote: 194
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog bundle2=* (glob)
  remote: 1
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  checking for updated bookmarks
  1 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" 250 bytes payload
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 0 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  * unknown header type b2x:infinitepush, backtrace:* (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Pushbackup fails too
  $ hgmn pushbackup ssh://user@dummy/repo --debug
  starting backup* (glob)
  running * (glob)
  sending hello command
  sending between command
  remote: 194
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog bundle2=* (glob)
  remote: 1
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  1 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 4 parts total
  bundle2-output-part: "replycaps" 250 bytes payload
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 0 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-output-part: "B2X:INFINITEPUSHSCRATCHBOOKMARKS" 459 bytes payload
  * unknown header type b2x:infinitepush, backtrace:* (glob)
  finished in * seconds (glob)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
