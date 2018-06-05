#if no-osx
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
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
  $ sleep 1 # Resolution of the database is in seconds. This avoid test flakiness
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 2 commits:
  remote:     67145f466344  initialcommit
  remote:     5ea4271ca0f0  commitwithbookmark
  finished in \d+\.(\d+)? seconds (re)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'SELECT bookmark, node, reponame from bookmarkstonode'
  bookmark	node	reponame
  infinitepush/backups/test/.*\$TESTTMP/client/bookmarks/abook	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)

Create a few more commits to test that pullbackup preserves order
  $ hg up -q 0
  $ mkcommit anothercommit > /dev/null
  $ hg pushbackup -q
  $ hg up -q 0
  $ sleep 1 # Resolution of the database is in seconds. This avoid test flakiness
  $ mkcommit anothercommit2 > /dev/null
  $ hg pushbackup -q
  $ hg log -T '{rev}:{node}\n'
  3:e1c1c1f2871f70bd24f941ebfec59f14adf7a13d
  2:f0d24965f49e87fc581a603dee76196f433444ff
  1:5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f
  0:67145f4663446a9580364f70034fea6e21293b6f

Pull backup and check that commits are in the same order
  $ cd ..
  $ hg clone -q ssh://user@dummy/server client2
  $ cd client2
  $ hg pullbackup -q
  $ hg log -T '{rev}:{node}\n'
  3:e1c1c1f2871f70bd24f941ebfec59f14adf7a13d
  2:f0d24965f49e87fc581a603dee76196f433444ff
  1:5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f
  0:67145f4663446a9580364f70034fea6e21293b6f

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
  remote: pushing 4 commits:
  remote:     67145f466344  initialcommit
  remote:     5ea4271ca0f0  commitwithbookmark
  remote:     f0d24965f49e  anothercommit
  remote:     e1c1c1f2871f  anothercommit2
  finished in \d+\.(\d+)? seconds (re)
  $ mysql -h $DBHOST -P $DBPORT -D $DBNAME -u $DBUSER $DBPASSOPT -e 'SELECT bookmark, node, reponame from bookmarkstonode'
  bookmark	node	reponame
  infinitepush/backups/test/.*\$TESTTMP/client/bookmarks/abook	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/e1c1c1f2871f70bd24f941ebfec59f14adf7a13d	e1c1c1f2871f70bd24f941ebfec59f14adf7a13d	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/f0d24965f49e87fc581a603dee76196f433444ff	f0d24965f49e87fc581a603dee76196f433444ff	babar (re)
  infinitepush/backups/test/.*\$TESTTMP/client/bookmarks/abook	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	newserver (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	5ea4271ca0f0cda5477241ae95ffc1fa7056ee6f	newserver (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/e1c1c1f2871f70bd24f941ebfec59f14adf7a13d	e1c1c1f2871f70bd24f941ebfec59f14adf7a13d	newserver (re)
  infinitepush/backups/test/.*\$TESTTMP/client/heads/f0d24965f49e87fc581a603dee76196f433444ff	f0d24965f49e87fc581a603dee76196f433444ff	newserver (re)
#endif
