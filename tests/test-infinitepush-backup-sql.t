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
  $ setupsqlserverhgrc
  $ setupdb
  $ cd ..
  $ hg clone -q ssh://user@dummy/server client
  $ cd client
  $ setupsqlclienthgrc
  $ mkcommit initialcommit
  $ hg debugbackup
  searching for changes
  remote: pushing 1 commit:
  remote:     67145f466344  initialcommit
  $ mkcommit commitwithbookmark
  $ hg book abook
  $ hg debugbackup
  searching for changes
  remote: pushing 2 commits:
  remote:     67145f466344  initialcommit
  remote:     5ea4271ca0f0  commitwithbookmark
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER -p"$DBPASS" -e 'SELECT bookmark, node from bookmarkstonode'
  bookmark	node
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/bookmarks/abook	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f (re)
  infinitepush/backups/test/[0-9a-zA-Z.-]+\$TESTTMP/client/heads/5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f (re)
#endif
