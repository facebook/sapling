#if no-osx
  $ extpath=`dirname $TESTDIR`
  $ cp -r $extpath/infinitepush $TESTTMP
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -d "0 0" -m "$1"
  > }
  $ setupclienthgrc() {
  > cat << EOF > $1/.hg/hgrc
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > [extensions]
  > infinitepush=$TESTTMP/infinitepush
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > server=False
  > [paths]
  > default = ssh://user@dummy/server
  > EOF
  > }
  $ setupserverhgrc() {
  > cat << EOF > $1/.hg/hgrc
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > [extensions]
  > infinitepush=$TESTTMP/infinitepush
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > server=True
  > indextype=sql
  > storetype=disk
  > EOF
  > }
  $ createdb() {
  > mysql -h $DBHOST -P $DBPORT -u $DBUSER -p"$DBPASS" -e "CREATE DATABASE IF NOT EXISTS $DBNAME;" 2>/dev/null
  > mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER -p"$DBPASS" -e '
  > DROP TABLE IF EXISTS nodestobundle;
  > DROP TABLE IF EXISTS bookmarkstonode;
  > DROP TABLE IF EXISTS bundles;
  > CREATE TABLE IF NOT EXISTS nodestobundle(
  > node CHAR(40) BINARY NOT NULL,
  > bundle VARCHAR(512) BINARY NOT NULL,
  > reponame CHAR(255) BINARY NOT NULL,
  > PRIMARY KEY(node, reponame));
  >  
  > CREATE TABLE IF NOT EXISTS bookmarkstonode(
  > node CHAR(40) BINARY NOT NULL,
  > bookmark VARCHAR(512) BINARY NOT NULL,
  > reponame CHAR(255) BINARY NOT NULL,
  > PRIMARY KEY(reponame, bookmark));
  > 
  > CREATE TABLE IF NOT EXISTS bundles(
  > bundle VARCHAR(512) BINARY NOT NULL,
  > reponame CHAR(255) BINARY NOT NULL,
  > PRIMARY KEY(bundle, reponame));' 2>/dev/null
  > }

With no configuration it should abort
  $ hg init server
  $ setupserverhgrc server
  $ cd server
  $ hg st
  abort: please set infinitepush.sqlhost
  [255]
  $ DBHOSTPORT=`$TESTDIR/getdb.sh` || exit 1
  $ echo "sqlhost=$DBHOSTPORT" >> .hg/hgrc
  $ echo "reponame=babar" >> .hg/hgrc
  $ DBHOST=`echo $DBHOSTPORT | cut -d : -f 1`
  $ DBPORT=`echo $DBHOSTPORT | cut -d : -f 2`
  $ DBNAME=`echo $DBHOSTPORT | cut -d : -f 3`
  $ DBUSER=`echo $DBHOSTPORT | cut -d : -f 4`
  $ DBPASS=`echo $DBHOSTPORT | cut -d : -f 5-`
  $ createdb
  $ cd ..
  $ hg clone -q --config ui.ssh='python "$TESTDIR/dummyssh"' ssh://user@dummy/server client1
  $ hg clone -q --config ui.ssh='python "$TESTDIR/dummyssh"' ssh://user@dummy/server client2
  $ setupclienthgrc client1
  $ setupclienthgrc client2
  $ cd client1
  $ mkcommit scratchcommit

  $ hg push -r . --to scratch/book --create
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     2d9cfa751213  scratchcommit

Make pull and check that scratch commit is not pulled
  $ cd ../client2
  $ hg pull
  pulling from ssh://user@dummy/server
  no changes found
  $ hg up scratch/book
  abort: unknown revision 'scratch/book'!
  [255]

Pull scratch commit from the second client
  $ hg pull -B scratch/book
  pulling from ssh://user@dummy/server
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up scratch/book
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark scratch/book)
  $ hg log -G
  @  changeset:   0:2d9cfa751213
     bookmark:    scratch/book
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     scratchcommit
  
  $ cd ../server
  $ hg book scratch/%erversidebook
  $ hg book serversidebook
  $ cd ../client1
  $ hg book --list-remote 'scratch/*'
     scratch/%erversidebook    0000000000000000000000000000000000000000
     scratch/book              2d9cfa7512136a84a6edb6a7c288145229c2ef7f
  $ hg book --list-remote 'scratch/%*'
     scratch/%erversidebook    0000000000000000000000000000000000000000
#endif
