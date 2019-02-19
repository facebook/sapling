  $ . "$TESTDIR/hgsql/library.sh"
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
  > if expected_book == actual_book and expected_head == actual_head:
  >     print "[ReplayVerification] Everything seems in order"
  >     sys.exit(0)
  > print "[ReplayVerification] Expected: (%s, %s). Actual: (%s, %s)" % (expected_book, expected_head, actual_book, actual_head)
  > sys.exit(1)
  > EOF

Setup a server repo
  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc <<CONFIG
  > [treemanifest]
  > server = True
  > [remotefilelog]
  > server = True
  > shallowtrees = True
  > CONFIG
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF
  $ hg bookmark master_bookmark -r tip
  $ hg phase -r "all()" --public
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
  > a0c9c57910584da709d7f4ed9852d66693a45ba7=0 0
  > EOF
  $ cat >>$TESTTMP/badcommitdates <<EOF
  > a0c9c57910584da709d7f4ed9852d66693a45ba7=1 0
  > EOF

Send unbundlereplay with incorrect expected hash
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server --debug -r d2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark <$TESTTMP/goodcommitdates
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 527
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Expected: (master_bookmark, d2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed

Send unbundlereplay with incorrect expected bookmark
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server --debug -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark_2 <$TESTTMP/goodcommitdates
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 527
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Expected: (master_bookmark_2, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed

Send unbundlereplay with incorrect commit timestamp
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server --debug -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark <$TESTTMP/badcommitdates
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 527
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Expected: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, 893d83f11bf81ce2b895a93d51638d4049d56ce2)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed

Send Unbundlereplay
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server --debug -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark <$TESTTMP/goodcommitdates
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 527
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size 309
  bundle2-input-part: total payload size 85

What is the new server state?
  $ log -r "all()"
  o  1 [public;rev=3;c2e526aacb51] master_bookmark
  |
  o  C [public;rev=2;26805aba1e60]
  |
  o  B [public;rev=1;112478962961]
  |
  o  A [public;rev=0;426bada5c675]
  
Let us set up another servevr repo, this time hgsql
  $ cd ..
  $ initserver server-hgsql server
  $ cd server-hgsql
  $ cat >> .hg/hgrc <<CONFIG
  > [treemanifest]
  > server = True
  > [remotefilelog]
  > server = True
  > shallowtrees = True
  > CONFIG
  $ DBGD=1 hg backfilltree
-- let's populate the hgsql server with some initial commits by pushing
  $ cd ..
  $ hg init hgsql-client && cd hgsql-client
  $ cat >>.hg/hgrc <<CONFIG
  > [paths]
  > default=ssh://user@dummy/server-hgsql
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
  $ hg push --to master_bookmark --create -r tip -q
  $ cd ../server-hgsql
  $ log -r "all()"
  o  C [public;rev=2;26805aba1e60] master_bookmark
  |
  o  B [public;rev=1;112478962961]
  |
  o  A [public;rev=0;426bada5c675]
  
-- let's set up the hook again
  $ cat >>.hg/hgrc <<CONFIG
  > [hooks]
  > prepushkey = python "$TESTTMP/replayverification.py"
  > CONFIG

Send unbundlereplay with incorrect expected hash to hgsql server
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server-hgsql --debug -r d2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark <$TESTTMP/goodcommitdates
  running * 'user@dummy' 'hg -R server-hgsql serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 544
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Expected: (master_bookmark, d2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed

Send unbundlereplay with incorrect expected bookmark to hgsql server
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server-hgsql --debug -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark_2 <$TESTTMP/goodcommitdates
  running * 'user@dummy' 'hg -R server-hgsql serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 544
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Expected: (master_bookmark_2, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed

Send unbundlereplay with incorrect commit timestamp to hgsql server
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server-hgsql --debug -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark <$TESTTMP/badcommitdates
  running * 'user@dummy' 'hg -R server-hgsql serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 544
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Expected: (master_bookmark, c2e526aacb5100b7c1ddb9b711d2e012e6c69cda). Actual: (master_bookmark, 893d83f11bf81ce2b895a93d51638d4049d56ce2)
  remote: pushkey-abort: prepushkey hook exited with status 1
  remote: transaction abort!
  remote: rollback completed

Send correct unbundlereplay to hgsql server
  $ hg sendunbundlereplay --file $TESTDIR/bundles/sendunbundle.test.hg --path ssh://user@dummy/server-hgsql --debug --traceback -r c2e526aacb5100b7c1ddb9b711d2e012e6c69cda -b master_bookmark <$TESTTMP/goodcommitdates
  running * 'user@dummy' 'hg -R server-hgsql serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 544
  remote: capabilities:* unbundlereplay* (glob)
  remote: 1
  sending unbundlereplay command
  remote: pushing 1 changeset:
  remote:     a0c9c5791058  1
  remote: 1 new changeset from the server will be downloaded
  remote: [ReplayVerification] Everything seems in order
  bundle2-input-part: total payload size 309
  bundle2-input-part: total payload size 85
