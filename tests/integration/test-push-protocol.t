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

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo3
  $ cd repo2
  $ hg pull ../repo-hg
  pulling from ../repo-hg
  searching for changes
  no changes found

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ wait_for_mononoke $TESTTMP/repo

create a new commit in repo2 and check that it's seen as outgoing

  $ mkdir b_dir
  $ touch b_dir/b
  $ hg add b_dir/b
  $ hg ci -mb
  $ hg log
  changeset:   1:8eea60339f0d
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
  changeset:   1:8eea60339f0d
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
  8eea60339f0d60ba0f3bdca74dfd11b1a281f321
  sending unbundle command
  bundle2-output-bundle: "HG20", 4 parts total
  bundle2-output-part: "replycaps" 196 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total

Now pull what was just pushed TODO(T25252425) make this work
  $ cd ../repo3
  $ hgmn pull -q
  devel-warn: applied empty changegroup at: * (glob)
  $ hg log -r 0e067c57feba
  abort: unknown revision '0e067c57feba'!
  [255]
