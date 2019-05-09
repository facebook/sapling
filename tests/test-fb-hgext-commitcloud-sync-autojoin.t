  $ . $TESTDIR/infinitepush/library.sh
  $ enable amend directaccess commitcloud infinitepush infinitepushbackup share
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ setconfig commitcloud.hostname=testhost
  $ cat > $TESTTMP/.commitcloudrc <<EOF
  > [commitcloud]
  > user_token=xxxxx
  > EOF
  $ newrepo server
  $ setconfig infinitepush.server=yes infinitepush.indextype=disk  infinitepush.storetype=disk
  $ setconfig infinitepush.reponame=testrepo
  $ echo base > base
  $ hg commit -Aqm base
  $ cd $TESTTMP
  $ hg clone ssh://user@dummy/server client1 -q
  $ cd client1
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation="$TESTTMP"
  $ setconfig commitcloud.user_token_path="$TESTTMP"

Normally pushbackup doesn't connect to commit cloud sync
  $ hg pushbackup --background
  $ waitbgbackup
  $ test -f .hg/store/commitcloudrc
  [1]

Set autocloud join, now pushbackup does connect to commit cloud sync
  $ setconfig commitcloud.autocloudjoin=true
  $ hg pushbackup --background
  $ waitbgbackup
  $ cat .hg/store/commitcloudrc
  [commitcloud]
  current_workspace=user/test/default

Deliberately disconnect.  Auto cloud join shouldn't make us reconect.
  $ hg cloud leave
  commitcloud: this repository is now disconnected from commit cloud
  $ cat .hg/store/commitcloudrc
  [commitcloud]
  disconnected=true
  $ hg pushbackup --background
  $ waitbgbackup
  $ cat .hg/store/commitcloudrc
  [commitcloud]
  disconnected=true

But we can manually reconnect
  $ hg cloud join
  commitcloud: this repository is now connected to the 'user/test/default' workspace for the 'server' repo
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: commits synchronized
  finished in * (glob)
  $ cat .hg/store/commitcloudrc
  [commitcloud]
  current_workspace=user/test/default

