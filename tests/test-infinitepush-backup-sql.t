#if no-osx
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ hg init server
  $ cd server
  $ setupsqlserverhgrc babar
  $ setupdb
  $ cd ..
  $ hg clone -q ssh://user@dummy/server client
  $ cd client
  $ setupsqlclienthgrc
  $ mkcommit initialcommit
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  finished in \d+\.(\d+)? seconds (re)
  $ mkcommit commitwithbookmark
  $ hg book abook
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     67145f466344  initialcommit
  remote:     5ea4271ca0f0  commitwithbookmark
  finished in \d+\.(\d+)? seconds (re)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER -p"$DBPASS" -e 'SELECT bookmark, node, reponame from bookmarkstonode'
  bookmark	node	reponame
  infinitepush/backups/test/.*\$TESTTMP/client/bookmarks/abook	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)
Create a server with different name that connects to the same db
  $ cd ..
  $ rm -rf server
  $ hg init server
  $ cd server
  $ setupsqlserverhgrc newserver
  $ echo "sqlhost=$DBHOST:$DBPORT:$DBNAME:$DBUSER:$DBPASS" >> .hg/hgrc

Go to client, delete backup state and run pushbackup. Make sure that it doesn't delete entries from another repo
  $ cd ../client
  $ rm .hg/infinitepushbackupstate
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     67145f466344  initialcommit
  remote:     5ea4271ca0f0  commitwithbookmark
  finished in \d+\.(\d+)? seconds (re)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER -p"$DBPASS" -e 'SELECT bookmark, node, reponame from bookmarkstonode'
  bookmark	node	reponame
  infinitepush/backups/test/.*\$TESTTMP/client/bookmarks/abook	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/bookmarks/abook	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	newserver (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	newserver (re)
#endif
