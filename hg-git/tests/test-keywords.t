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

This commit is called gamma10 so that its hash will have the same initial digit
as commit alpha. This lets us test ambiguous abbreviated identifiers.

  $ echo gamma10 > gamma10
  $ git add gamma10
  $ fn_git_commit -m 'add gamma10'

  $ cd ..

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ echo gamma > gamma
  $ hg add gamma
  $ hg commit -m 'add gamma'

  $ hg log --template "{rev} {node} {node|short} {gitnode} {gitnode|short}\n"
  3 965bf7d08d3ac847dd8eb9e72ee0bf547d1a65d9 965bf7d08d3a  
  2 8e3f0ecc9aefd4ea2fdf8e2d5299cac548762a1c 8e3f0ecc9aef 7e2a5465ff4e3b992c429bb87a392620a0ac97b7 7e2a5465ff4e
  1 7fe02317c63d9ee324d4b5df7c9296085162da1b 7fe02317c63d 9497a4ee62e16ee641860d7677cdb2589ea15554 9497a4ee62e1
  0 ff7a2f2d8d7099694ae1e8b03838d40575bebb63 ff7a2f2d8d70 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 7eeab2ea75ec
  $ hg log --template "fromgit {rev}\n" --rev "fromgit()"
  fromgit 0
  fromgit 1
  fromgit 2
  $ hg log --template "gitnode_existsA {rev}\n" --rev "gitnode(9497a4ee62e16ee641860d7677cdb2589ea15554)"
  gitnode_existsA 1
  $ hg log --template "gitnode_existsB {rev}\n" --rev "gitnode(7eeab)"
  gitnode_existsB 0
  $ hg log --rev "gitnode(7e)"
  abort: git-mapfile@7e: ambiguous identifier!
  [255]
  $ hg log --template "gitnode_notexists {rev}\n" --rev "gitnode(1234567890ab)"
