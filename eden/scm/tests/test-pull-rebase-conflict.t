#require no-eden

  $ configure mutation-norecord
  $ enable rebase amend fbcodereview commitcloud
  $ setconfig fbcodereview.allow-diff-revision-drop=true
  $ setconfig experimental.rebaseskipobsolete=true
  $ setconfig extensions.arcconfig="$TESTDIR/../sapling/ext/extlib/phabricator/arcconfig.py"

  $ echo '{}' > $TESTTMP/.arcrc
  $ echo '{"config" : {"default" : "https://a.com/api"}, "hosts" : {"https://a.com/api/" : { "user" : "testuser", "oauth" : "garbage_cert"}}}' > $TESTTMP/.arcconfig

  $ export HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit

  $ landed_graphql() {
  >   printf '{"number": %s, ' $1
  >   printf '"diff_status_name": "Closed",'
  >   printf '"phabricator_versions": { "nodes": [] }, "phabricator_diff_commit": '
  >   printf '{ "nodes": [{"commit_identifier": "%s"}]}}' $2
  > }

  $ commitdrev() {
  >   printf "%s\n\nDifferential Revision: https://phabricator.fb.com/D%s" "$1" "$2" > $TESTTMP/msg
  >   sl commit -ql $TESTTMP/msg
  > }

Set up the server repo

  $ newrepo server
  $ echo base > file
  $ sl commit -Aqm "base"
  $ sl bookmark master
  $ cd ..

Clone the client

  $ sl clone -q test:server client
  $ cd client
  $ cp $TESTTMP/.arcconfig .

Create a draft commit and upload it to the server

  $ echo "draft content" > file
  $ commitdrev "my change" 123
  $ DRAFT=$(sl log -r . -T '{node}')
  $ sl cloud upload -q -r .

Land the draft on the server by rebasing onto master.

  $ cd ../server
  $ sl commit -Aqm "another server commit" --config ui.allowemptycommit=true
  $ sl bookmark -f master
  $ sl rebase -q -r $DRAFT -d master
  $ sl bookmark -f master -r tip
  $ LANDED=$(sl log -r tip -T '{node}')

Add a conflicting commit on the server on top of the landed commit

  $ sl goto -q tip
  $ echo "more server work" > file
  $ sl commit -Aqm "server change"
  $ sl bookmark -f master
  $ cd ../client

  $ printf '[{"data": {"phabricator_diff_query": [{"results": {"nodes": [%s]}}]}}]' \
  > "$(landed_graphql 123 $LANDED)" > $TESTTMP/mockduit

Now pull --rebase with the mock. The _marklanded call between pull and
rebase detects the landed commit, so the rebase skips it instead of
conflicting.

  $ HG_ARC_CONDUIT_MOCK=$TESTTMP/mockduit sl pull -q --rebase
