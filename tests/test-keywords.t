Load commonly used test logic
  $ . "$TESTDIR/testutil"

  $ git init gitrepo
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'

  $ cd ..

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ echo gamma > gamma
  $ hg add gamma
  $ hg commit -m 'add gamma'

  $ hg log --template "{rev} {node} {node|short} {gitnode} {gitnode|short}\n"
  2 168eb1ee8b3c04e6723c9330327b0eec1e36577f 168eb1ee8b3c  
  1 7fe02317c63d9ee324d4b5df7c9296085162da1b 7fe02317c63d 9497a4ee62e16ee641860d7677cdb2589ea15554 9497a4ee62e1
  0 ff7a2f2d8d7099694ae1e8b03838d40575bebb63 ff7a2f2d8d70 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 7eeab2ea75ec
  $ hg log --template "fromgit {rev}\n" --rev "fromgit()"
  fromgit 0
  fromgit 1
  $ hg log --template "gitnode_existsA {rev}\n" --rev "gitnode(9497a4ee62e16ee641860d7677cdb2589ea15554)"
  gitnode_existsA 1
  $ hg log --template "gitnode_existsB {rev}\n" --rev "gitnode(7eeab2ea75ec)"
  gitnode_existsB 0
  $ hg log --template "gitnode_notexists {rev}\n" --rev "gitnode(1234567890ab)"
