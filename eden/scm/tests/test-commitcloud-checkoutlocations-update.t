#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ configure dummyssh
  $ enable infinitepush commitcloud
  $ enable amend
  $ setconfig infinitepush.branchpattern="re:scratch/.*"
  $ setconfig commitcloud.hostname=testhost
  $ setconfig experimental.graphstyle.grandparent=2.
  $ setconfig templatealias.sl_cloud="\"{truncatelonglines(node, 6)} {ifeq(phase, 'public', '(public)', '')} {ifeq(phase, 'draft', author, '')} {date|isodate} {bookmarks}\\n{desc|firstline}\\n \""

  $ setconfig remotefilelog.reponame=server

  $ newserver server
  $ cd ..

Make the clone of the server
  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation="$TESTTMP" commitcloud.token_enforced=False

Enable syncing checkout locations
  $ setconfig commitcloud.synccheckoutlocations=True

Utility script to dump json of the checkoutlocation being sent
  $ cat > $TESTTMP/dumpdata.py <<EOF
  > import json
  > import os, sys
  > testtmp = os.environ['TESTTMP']
  > path = os.path.join(testtmp, "checkoutlocations")
  > location = json.load(open(path))
  > def println(s):  # avoid CRLF on Windows
  >     sys.stdout.buffer.write(f"{s}\n".encode())
  > println("repo_name: %s" % location["repo_name"])
  > println("workspace: %s" % location["workspace"])
  > println("hostname: %s" % location["hostname"])
  > println("commit: %s" % location["commit"])
  > println("shared_path: %s" % location["shared_path"])
  > println("checkout_path: %s" % location["checkout_path"])
  > println("unixname: %s" % location["unixname"])
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
  shared_path: $TESTTMP/client/.hg
  checkout_path: $TESTTMP/client/.hg
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
  shared_path: $TESTTMP/client/.hg
  checkout_path: $TESTTMP/client/.hg
  unixname: test

Checkout the old commit and see if the location is synced
  $ hg checkout bb757c825e81d15d6959648d8f055c8e5958310f --hidden
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ python $TESTTMP/dumpdata.py
  repo_name: server
  workspace: user/test/default
  hostname: * (glob)
  commit: bb757c825e81d15d6959648d8f055c8e5958310f
  shared_path: $TESTTMP/client/.hg
  checkout_path: $TESTTMP/client/.hg
  unixname: test
