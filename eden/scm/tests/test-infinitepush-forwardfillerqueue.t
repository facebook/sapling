#chg-compatible

#if no-windows no-osx
  $ disable treemanifest
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
  $ cd ..

Without forwardfill, it should not insert into the queue
  $ hg clone -q ssh://user@dummy/server client1
  $ cd client1
  $ setupsqlclienthgrc
  $ mkcommit commit1
  $ hg push -r . --to scratch/book --create
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     cb9a30b04b9d  commit1
  $ cd ..

Enable forwardfill on the server
  $ cd server
  $ enableforwardfill
  $ cd ..

With forwardfill, it should insert into the queue
  $ hg clone -q ssh://user@dummy/server client2
  $ cd client2
  $ setupsqlclienthgrc
  $ mkcommit commit2
  $ hg push -r . --to scratch/book2 --create
  pushing to ssh://user@dummy/server
  searching for changes
  remote: pushing 1 commit:
  remote:     6fdf683f5af9  commit2
  $ cd ..

Proper metadata should have been recorded
  $ querysqlindex "SELECT * FROM nodestobundle;"
  node	bundle	reponame
  6fdf683f5af9a2be091b81ef475f335e2624fb0d	8347a06785e3bdd572ebeb7df3aac1356acb4ce5	repo123
  cb9a30b04b9df854f40d21fdac525408f3bd6c78	944fe1c133f63c7711aa15db2dd9216084dacc36	repo123
  $ querysqlindex "SELECT id, reponame, bundle FROM forwardfillerqueue;"
  id	reponame	bundle
  1	repo123	8347a06785e3bdd572ebeb7df3aac1356acb4ce5
#endif
