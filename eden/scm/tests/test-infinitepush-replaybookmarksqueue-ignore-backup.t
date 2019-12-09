#chg-compatible

#if no-windows no-osx
  $ setconfig extensions.treemanifest=!
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -d "0 0" -m "$1"
  > }
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon

Configure the server
  $ hg init server
  $ cd server
  $ setupsqlserverhgrc repo123
  $ setupdb
  $ enablereplaybookmarks
  $ cd ..

It should backup many bookmarks
  $ hg clone -q ssh://user@dummy/server client
  $ cd client
  $ setupsqlclienthgrc
  $ mkcommit commit0
  $ commit0="$(hg id -i)"
  $ hg up -q "$commit0" && mkcommit commit1
  $ hg up -q "$commit0" && mkcommit commit2
  $ hg up -q "$commit0" && mkcommit commit3
  $ hg cloud backup
  backing up stack rooted at ace906b76ab4
  remote: pushing 4 commits:
  remote:     ace906b76ab4  commit0
  remote:     b1e07bb9979c  commit1
  remote:     33701f08790f  commit2
  remote:     db45a2d42cf6  commit3
  commitcloud: backed up 4 commits
  $ hg push -r . --to scratch/book --create
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 2 commits:
  remote:     ace906b76ab4  commit0
  remote:     db45a2d42cf6  commit3
  $ cd ..

Backups should be excluded
  $ querysqlindex "SELECT * FROM nodestobundle;"
  node	bundle	reponame
  33701f08790f0e038ca262ddb72728754f60ec88	318c3749b9def132140c54c6f84e853855aa5042	repo123
  ace906b76ab45ac794eb67142e1466725def57cb	318c3749b9def132140c54c6f84e853855aa5042	repo123
  b1e07bb9979cf151ae6a05d0cd9008737c77dfea	318c3749b9def132140c54c6f84e853855aa5042	repo123
  db45a2d42cf67d746ba59e17f09df3eb9e8c2f4c	318c3749b9def132140c54c6f84e853855aa5042	repo123
  $ querysqlindex "SELECT reponame, synced, bookmark, node FROM replaybookmarksqueue;"
  reponame	synced	bookmark	node
  repo123	0	scratch/book	db45a2d42cf67d746ba59e17f09df3eb9e8c2f4c
  $ querysqlindex "SELECT reponame, bookmark, node FROM bookmarkstonode ORDER BY node ASC;"
  reponame	bookmark	node
  repo123	infinitepush/backups/test/*/client/heads/33701f08790f0e038ca262ddb72728754f60ec88	33701f08790f0e038ca262ddb72728754f60ec88 (glob)
  repo123	infinitepush/backups/test/*/client/heads/b1e07bb9979cf151ae6a05d0cd9008737c77dfea	b1e07bb9979cf151ae6a05d0cd9008737c77dfea (glob)
  repo123	infinitepush/backups/test/*/client/heads/db45a2d42cf67d746ba59e17f09df3eb9e8c2f4c	db45a2d42cf67d746ba59e17f09df3eb9e8c2f4c (glob)
  repo123	scratch/book	db45a2d42cf67d746ba59e17f09df3eb9e8c2f4c
#endif
