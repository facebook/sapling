  $ . "$TESTDIR/hgsql/library.sh"
#testcases respondlightly respondfully

Do some initial setup
  $ CACHEDIR=`pwd`/hgcache
  $ cat >> $HGRCPATH <<CONFIG
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > sendunbundlereplay=
  > smartlog=
  > treemanifest=
  > fastmanifest=
  > remotefilelog=
  > pushrebase=
  > [remotefilelog]
  > reponame=testrepo
  > cachepath=$CACHEDIR
  > CONFIG

Setup helpers
  $ log() {
  >   hg sl -T "{desc} [{phase};rev={rev};{node|short}] {bookmarks}" "$@"
  > }

Implement a basic verification hook
  $ cat >>$TESTTMP/replayverification.py <<EOF
  > import os, sys
  > expected_book = os.environ["HG_EXPECTED_ONTOBOOK"]
  > expected_head = os.environ["HG_EXPECTED_REBASEDHEAD"]
  > actual_book = os.environ["HG_KEY"]
  > actual_head = os.environ["HG_NEW"]
  > if expected_book == actual_book:
  >     if ((expected_head is None and actual_head is None) or
  >           (expected_head == actual_head)):
  >       print "[ReplayVerification] Everything seems in order"
  >       sys.exit(0)
  > print "[ReplayVerification] Expected: (%s, %s). Actual: (%s, %s)" % (expected_book, expected_head, actual_book, actual_head)
  > sys.exit(1)
  > EOF

Setup a server repo
  $ initserver server server
  $ cd server
  $ cat >> .hg/hgrc <<CONFIG
  > [treemanifest]
  > server = True
  > [remotefilelog]
  > server = True
  > shallowtrees = True
  > CONFIG
  $ DBGD=1 hg backfilltree
  $ cd ..
  $ hg init hgsql-client-tmp && cd hgsql-client-tmp
  $ cat >>.hg/hgrc <<CONFIG
  > [paths]
  > default=ssh://user@dummy/server
  > [extensions]
  > remotenames=
  > CONFIG
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

  $ hg bookmark master_bookmark -r tip
  $ hg push --to master_bookmark --create -r tip -q
  $ cd ../server
  $ log -r "all()"
  o  C [public;rev=2;26805aba1e60] master_bookmark
  |
  o  B [public;rev=1;112478962961]
  |
  o  A [public;rev=0;426bada5c675]
  
  $ cat >>.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python "$TESTTMP/replayverification.py"
  > CONFIG

  $ cat >>$TESTTMP/goodcommitdates <<EOF
  > a0c9c57910584da709d7f4ed9852d66693a45ba7=0
  > c9b2673d32182756f799beff4ee8dc6a28645167=0
  > e91cd89a81a52269b7767c800db21e62b9cf98db=0
  > a6074953400c1969019122d5a6dad626b2da082b=0
  > cba0370ec397f4de9cbd83329410369a1d30575f=0
  > 6c384628f7c4fe3b7e89ed2ed382be72bf234c40=0
  > d5313099c10db8d9efee0f2aae13aeed4ab4c2ef=0
  > EOF
  $ cat >>$TESTTMP/badcommitdates <<EOF
  > a0c9c57910584da709d7f4ed9852d66693a45ba7=1
  > EOF

Send unbundlereplay with incorrect expected hash
  $ cat >$TESTTMP/commands <<EOF
  > $TESTDIR/bundles/sendunbundle.test.hg $TESTTMP/goodcommitdates master_bookmark d2e526aacb5100b7c1ddb9b711d2e012e6c69cda
  > EOF
  $ cat $TESTTMP/commands | hg sendunbundlereplaybatch --path ssh://user@dummy/server --debug --reports $TESTTMP/reports.txt
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  creating a peer took: * (glob)
  using $TESTTMP/reports.txt as a reports file
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: [ReplayVerification] Expected: (master_bookmark, d2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed
  single wireproto command took: * (glob)
  replay failed: error:pushkey
  unbundle replay batch item #0 failed
  [1]
  $ cat $TESTTMP/reports.txt
  unbundle replay batch item #0 failed

Send unbundlereplay with incorrect expected bookmark
  $ cat >$TESTTMP/commands <<EOF
  > $TESTDIR/bundles/sendunbundle.test.hg $TESTTMP/goodcommitdates master_bookmark_2 c2e526aacb5100b7c1ddb9b711d2e012e6c69cda
  > EOF
  $ cat $TESTTMP/commands | hg sendunbundlereplaybatch --path ssh://user@dummy/server --debug --reports $TESTTMP/reports.txt
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  creating a peer took: * (glob)
  using $TESTTMP/reports.txt as a reports file
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: [ReplayVerification] Expected: (master_bookmark_2, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed
  single wireproto command took: * (glob)
  replay failed: error:pushkey
  unbundle replay batch item #0 failed
  [1]
  $ cat $TESTTMP/reports.txt
  unbundle replay batch item #0 failed

Send unbundlereplay with incorrect commit timestamp
  $ cat >$TESTTMP/commands <<EOF
  > $TESTDIR/bundles/sendunbundle.test.hg $TESTTMP/badcommitdates master_bookmark c2e526aacb5100b7c1ddb9b711d2e012e6c69cda
  > EOF
  $ cat $TESTTMP/commands | hg sendunbundlereplaybatch --path ssh://user@dummy/server --debug --reports $TESTTMP/reports.txt
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  creating a peer took: * (glob)
  using $TESTTMP/reports.txt as a reports file
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: [ReplayVerification] Expected: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, 893d83f11bf81ce2b895a93d51638d4049d56ce2)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed
  single wireproto command took: * (glob)
  replay failed: error:pushkey
  unbundle replay batch item #0 failed
  [1]
  $ cat $TESTTMP/reports.txt
  unbundle replay batch item #0 failed

Send Unbundlereplay batch 1 (all good)
  $ cat >$TESTTMP/commands <<EOF
  > $TESTDIR/bundles/unbundlereplay/1.a0c9c57910584da709d7f4ed9852d66693a45ba7-c2e526aacb5100b7c1ddb9b711d2e012e6c69cda.hg $TESTTMP/goodcommitdates master_bookmark c2e526aacb5100b7c1ddb9b711d2e012e6c69cda
  > $TESTDIR/bundles/unbundlereplay/2.c9b2673d32182756f799beff4ee8dc6a28645167-dc31470c83861b8cc93ed4fa1376a0db0daab236.hg $TESTTMP/goodcommitdates master_bookmark dc31470c83861b8cc93ed4fa1376a0db0daab236
  > $TESTDIR/bundles/unbundlereplay/3.e91cd89a81a52269b7767c800db21e62b9cf98db-6398085ceb9d425db206d33688a70d5f442304f0.hg $TESTTMP/goodcommitdates master_bookmark 6398085ceb9d425db206d33688a70d5f442304f0
  > EOF
  $ cat $TESTTMP/commands | hg sendunbundlereplaybatch --reports $TESTTMP/reports.txt --path ssh://user@dummy/server --debug
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  creating a peer took: * (glob)
  using $TESTTMP/reports.txt as a reports file
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  unbundle replay batch item #0 successfully sent
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     c9b2673d3218  2
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  unbundle replay batch item #1 successfully sent
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     e91cd89a81a5  3
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  unbundle replay batch item #2 successfully sent
  $ cat $TESTTMP/reports.txt
  unbundle replay batch item #0 successfully sent
  unbundle replay batch item #1 successfully sent
  unbundle replay batch item #2 successfully sent

Send unbundlereplay batch 2 (second has a wrong hash)
  $ cat >$TESTTMP/commands <<EOF
  > $TESTDIR/bundles/unbundlereplay/4.a6074953400c1969019122d5a6dad626b2da082b-640744a246b11e91de1b915e3f155e4659b34ae2.hg $TESTTMP/goodcommitdates master_bookmark 640744a246b11e91de1b915e3f155e4659b34ae2
  > $TESTDIR/bundles/unbundlereplay/5.cba0370ec397f4de9cbd83329410369a1d30575f-cc43a8d5ff4cfd07429374cd22d8d2c94d030807.hg $TESTTMP/goodcommitdates master_bookmark 0000000000000000000000000000000000000000
  > $TESTDIR/bundles/unbundlereplay/6.6c384628f7c4fe3b7e89ed2ed382be72bf234c40-a976d3914119f1d620636098b7aeee7ae52ecefc.hg $TESTTMP/goodcommitdates master_bookmark a976d3914119f1d620636098b7aeee7ae52ecefc
  > EOF
  $ cat $TESTTMP/commands | hg sendunbundlereplaybatch --path ssh://user@dummy/server --debug --reports $TESTTMP/reports.txt
  running python * 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  creating a peer took: * (glob)
  using $TESTTMP/reports.txt as a reports file
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a6074953400c  4
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  unbundle replay batch item #0 successfully sent
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     cba0370ec397  5
  remote: [ReplayVerification] Expected: (master_bookmark, 0000000000000000000000000000000000000000). Actual: (master_bookmark, cc43a8d5ff4cfd07429374cd22d8d2c94d030807)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed
  single wireproto command took: * (glob)
  replay failed: error:pushkey
  unbundle replay batch item #1 failed
  [1]
  $ cat $TESTTMP/reports.txt
  unbundle replay batch item #0 successfully sent
  unbundle replay batch item #1 failed

#if respondlightly
Send unbundlereplay batch 3 (all good, this time with logging to files)
  $ cat >$TESTTMP/commands <<EOF
  > $TESTDIR/bundles/unbundlereplay/5.cba0370ec397f4de9cbd83329410369a1d30575f-cc43a8d5ff4cfd07429374cd22d8d2c94d030807.hg $TESTTMP/goodcommitdates master_bookmark cc43a8d5ff4cfd07429374cd22d8d2c94d030807 $TESTTMP/log1
  > $TESTDIR/bundles/unbundlereplay/6.6c384628f7c4fe3b7e89ed2ed382be72bf234c40-a976d3914119f1d620636098b7aeee7ae52ecefc.hg $TESTTMP/goodcommitdates master_bookmark a976d3914119f1d620636098b7aeee7ae52ecefc $TESTTMP/log2
  > $TESTDIR/bundles/unbundlereplay/7.d5313099c10db8d9efee0f2aae13aeed4ab4c2ef-0ee63ce2db781f5a2a2e1a2e063261e2b049011d.hg $TESTTMP/goodcommitdates master_bookmark 0ee63ce2db781f5a2a2e1a2e063261e2b049011d $TESTTMP/log3
  > EOF
  $ cat $TESTTMP/commands | hg sendunbundlereplaybatch --path ssh://user@dummy/server \
  > --debug --reports $TESTTMP/reports.txt
  running python * 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  creating a peer took: * (glob)
  using $TESTTMP/reports.txt as a reports file
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     cba0370ec397  5
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  unbundle replay batch item #0 successfully sent
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     6c384628f7c4  6
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  unbundle replay batch item #1 successfully sent
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     d5313099c10d  7
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  unbundle replay batch item #2 successfully sent
  $ cat $TESTTMP/log1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     cba0370ec397  5
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  $ cat $TESTTMP/log2
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     6c384628f7c4  6
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  $ cat $TESTTMP/log3
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     d5313099c10d  7
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
#endif

#if respondfully
Send unbundlereplay batch 3 (all good, this time with logging to files)
  $ cat >$TESTTMP/commands <<EOF
  > $TESTDIR/bundles/unbundlereplay/5.cba0370ec397f4de9cbd83329410369a1d30575f-cc43a8d5ff4cfd07429374cd22d8d2c94d030807.hg $TESTTMP/goodcommitdates master_bookmark cc43a8d5ff4cfd07429374cd22d8d2c94d030807 $TESTTMP/log1
  > $TESTDIR/bundles/unbundlereplay/6.6c384628f7c4fe3b7e89ed2ed382be72bf234c40-a976d3914119f1d620636098b7aeee7ae52ecefc.hg $TESTTMP/goodcommitdates master_bookmark a976d3914119f1d620636098b7aeee7ae52ecefc $TESTTMP/log2
  > $TESTDIR/bundles/unbundlereplay/7.d5313099c10db8d9efee0f2aae13aeed4ab4c2ef-0ee63ce2db781f5a2a2e1a2e063261e2b049011d.hg $TESTTMP/goodcommitdates master_bookmark 0ee63ce2db781f5a2a2e1a2e063261e2b049011d $TESTTMP/log3
  > EOF
  $ cat $TESTTMP/commands | hg sendunbundlereplaybatch --path ssh://user@dummy/server \
  > --debug --reports $TESTTMP/reports.txt \
  > --config sendunbundlereplay.respondlightly=off
  running python * 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  creating a peer took: * (glob)
  using $TESTTMP/reports.txt as a reports file
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     cba0370ec397  5
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: total payload size * (glob)
  single wireproto command took: * (glob)
  unbundle replay batch item #0 successfully sent
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     6c384628f7c4  6
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: total payload size * (glob)
  single wireproto command took: * (glob)
  unbundle replay batch item #1 successfully sent
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     d5313099c10d  7
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: total payload size * (glob)
  single wireproto command took: * (glob)
  unbundle replay batch item #2 successfully sent
  $ cat $TESTTMP/log1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     cba0370ec397  5
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: total payload size * (glob)
  single wireproto command took: * (glob)
  $ cat $TESTTMP/log2
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     6c384628f7c4  6
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: total payload size * (glob)
  single wireproto command took: * (glob)
  $ cat $TESTTMP/log3
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     d5313099c10d  7
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size * (glob)
  bundle2-input-part: total payload size * (glob)
  single wireproto command took: * (glob)
#endif

  $ cat $TESTTMP/reports.txt
  unbundle replay batch item #0 successfully sent
  unbundle replay batch item #1 successfully sent
  unbundle replay batch item #2 successfully sent

Send Unbundlereplay to delete a bookmark
  $ hg book newbook -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda
  $ hg book
     master_bookmark           9:0ee63ce2db78
     newbook                   3:c2e526aacb51
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle_delete_bookmark.test.hg --path ssh://user@dummy/server -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda --deleted -b newbook --debug
  abort: can't use `--rebasedhead` and `--deleted`
  [255]
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle_delete_bookmark.test.hg --path ssh://user@dummy/server --deleted -b newbook --debug
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  creating a peer took: * (glob)
  sending unbundlereplay command
  remote: [ReplayVerification] Everything seems in order
  single wireproto command took: * (glob)
  $ hg book
     master_bookmark           9:0ee63ce2db78

What is the new server state?
  $ log -r "all()"
  o  7 [public;rev=9;0ee63ce2db78] master_bookmark
  |
  o  6 [public;rev=8;a976d3914119]
  |
  o  5 [public;rev=7;cc43a8d5ff4c]
  |
  o  4 [public;rev=6;640744a246b1]
  |
  o  3 [public;rev=5;6398085ceb9d]
  |
  o  2 [public;rev=4;dc31470c8386]
  |
  o  1 [public;rev=3;c2e526aacb51]
  |
  o  C [public;rev=2;26805aba1e60]
  |
  o  B [public;rev=1;112478962961]
  |
  o  A [public;rev=0;426bada5c675]
  
