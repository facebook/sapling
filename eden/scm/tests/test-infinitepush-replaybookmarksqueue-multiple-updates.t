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

It should insert an entry for each update
  $ hg clone -q ssh://user@dummy/server client2
  $ cd client2
  $ setupsqlclienthgrc
  $ mkcommit commit2
  $ hg push -r . --to scratch/123 --create
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     6fdf683f5af9  commit2
  $ mkcommit commit3
  $ hg push -r . --to scratch/123
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 2 commits:
  remote:     6fdf683f5af9  commit2
  remote:     8e0c8ddac9fb  commit3
  $ mkcommit commit4
  $ hg push -r . --to scratch/123
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 3 commits:
  remote:     6fdf683f5af9  commit2
  remote:     8e0c8ddac9fb  commit3
  remote:     feccf85eaa94  commit4
  $ cd ..

Proper metadata should have been recorded
  $ querysqlindex "SELECT * FROM nodestobundle;"
  node	bundle	reponame
  6fdf683f5af9a2be091b81ef475f335e2624fb0d	f47f4ea5c9dade34f2a38376fe371dc6e4c49c1d	repo123
  8e0c8ddac9fb06e5cb0b3ca65a51632a7814f576	f47f4ea5c9dade34f2a38376fe371dc6e4c49c1d	repo123
  feccf85eaa94ff5ec0f80b8fd871d0fa3125a09b	f47f4ea5c9dade34f2a38376fe371dc6e4c49c1d	repo123
  $ querysqlindex "SELECT id, reponame, synced, bookmark, node, bookmark_hash FROM replaybookmarksqueue;"
  id	reponame	synced	bookmark	node	bookmark_hash
  1	repo123	0	scratch/123	6fdf683f5af9a2be091b81ef475f335e2624fb0d	68e2c1170bb6960df6ab9e2c7da427b5d3eca47e
  2	repo123	0	scratch/123	8e0c8ddac9fb06e5cb0b3ca65a51632a7814f576	68e2c1170bb6960df6ab9e2c7da427b5d3eca47e
  3	repo123	0	scratch/123	feccf85eaa94ff5ec0f80b8fd871d0fa3125a09b	68e2c1170bb6960df6ab9e2c7da427b5d3eca47e
#endif
