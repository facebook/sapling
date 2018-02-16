  $ . $TESTDIR/library.sh

setup configuration

  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
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
  $ blobimport --blobstore files --linknodes repo-hg repo

blobimport currently doesn't handle bookmarks, but server requires the directory.
  $ mkdir -p repo/books

Need a place for the socket to live
  $ mkdir -p repo/.hg

setup repo2

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2
  $ cd repo2
  $ hg pull ../repo-hg
  pulling from ../repo-hg
  searching for changes
  no changes found

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ wait_for_mononoke $TESTTMP/repo

create a new commit in repo2 and check that it's seen as outgoing

  $ touch b
  $ hg add b
  $ hg ci -mb
  $ hg log
  changeset:   1:0e067c57feba
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hgmn outgoing ssh://user@dummy/repo
  comparing with ssh://user@dummy/repo
  searching for changes
  changeset:   1:0e067c57feba
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  

push to Mononoke TODO(T25252425) make this work

  $ hgmn push --config treemanifest.treeonly=True --debug ssh://user@dummy/repo
  pushing to ssh://user@dummy/repo
  running *scm/mononoke/tests/integration/dummyssh.par 'user@dummy' ''\''*scm/mononoke/hgcli/hgcli#binary/hgcli'\'' -R repo serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 194
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog bundle2=HG20%0Alistkeys%0Achangegroup%3D02%0Ab2x%3Ainfinitepush%0Ab2x%3Ainfinitepushscratchbookmarks
  remote: 1
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  checking for updated bookmarks
  1 changesets found
  list of changesets:
  0e067c57feba1a5694ca4844f05588bb1bf82342
  sending unbundle command
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 196 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  error: invalid magic: '2\n' (version '0\n'), should be 'HG'
  abort: not a Mercurial bundle
  [255]
