  $ enable infinitepush commitcloud
  $ enable amend
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost
  $ setconfig experimental.graphstyle.grandparent=2.
  $ setconfig templatealias.sl_cloud="\"{truncatelonglines(node, 6)} {ifeq(phase, 'public', '(public)', '')} {ifeq(phase, 'draft', author, '')} {date|isodate} {bookmarks}\\n{desc|firstline}\\n \""

  $ setconfig remotefilelog.reponame=server

  $ hg init server
  $ cd server
  $ setconfig infinitepush.server=yes infinitepush.indextype=disk infinitepush.storetype=disk infinitepush.reponame=testrepo

Make the clone of the server
  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation="$TESTTMP" commitcloud.user_token_path="$TESTTMP"

Enable syncing checkout locations
  $ setconfig commitcloud.synccheckoutlocations=True

Registration
  $ hg cloud auth -t '****************'
  setting authentication token
  authentication successful

Utility script to dump json of the checkoutlocation being sent
  $ cat > $TESTTMP/dumpdata.py <<EOF
  > import json
  > import os
  > testtmp = os.environ['TESTTMP']
  > path = os.path.join(testtmp, "checkoutlocations")
  > location = json.load(open(path))
  > print("repo_name: %s" % location["repo_name"])
  > print("workspace: %s" % location["workspace"])
  > print("hostname: %s" % location["hostname"])
  > print("commit: %s" % location["commit"])
  > print("shared_path: %s" % location["shared_path"])
  > print("checkout_path: %s" % location["checkout_path"])
  > print("unixname: %s" % location["unixname"])
  > EOF

Make a random commit
  $ echo a > a
  $ hg add a
  $ hg commit -m"commit"

Check that the checkout locations are synced after the commit is made
  $ python $TESTTMP/dumpdata.py
  repo_name: server
  workspace: user/test/default
  hostname: * (glob)
  commit: bb757c825e81d15d6959648d8f055c8e5958310f
  shared_path: $TESTTMP/server/client/.hg
  checkout_path: $TESTTMP/server/client/.hg
  unixname: test

Make changes and amend
  $ echo aa > a
  $ hg amend

Check that the checkout locations are synced after the amend is made
  $ python $TESTTMP/dumpdata.py
  repo_name: server
  workspace: user/test/default
  hostname: * (glob)
  commit: b7ad20e4fc527a09952053de497603c0a8eafd0d
  shared_path: $TESTTMP/server/client/.hg
  checkout_path: $TESTTMP/server/client/.hg
  unixname: test

Checkout the old commit and see if the location is synced
  $ hg checkout bb757c825e81d15d6959648d8f055c8e5958310f --hidden
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ python $TESTTMP/dumpdata.py
  repo_name: server
  workspace: user/test/default
  hostname: * (glob)
  commit: bb757c825e81d15d6959648d8f055c8e5958310f
  shared_path: $TESTTMP/server/client/.hg
  checkout_path: $TESTTMP/server/client/.hg
  unixname: test
