Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

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

Remove the mapfile so we can ensure the gitnode is from the extras not the mapfile.

  $ mv .hg/git-mapfile .hg/git-mapfile-bak
  $ hg log --template "{rev} {node} {node|short} {gitnode} {gitnode|short}\n"
  3 f5172ebb976873f9e41d2958e3b665a985128b00 f5172ebb9768  
  2 fedf4edd982fb98273f2255d6b97c892ec208427 fedf4edd982f 7e2a5465ff4e3b992c429bb87a392620a0ac97b7 7e2a5465ff4e
  1 3bb02b6794ddc0b498cdc15f59f2e6724cabfa2f 3bb02b6794dd 9497a4ee62e16ee641860d7677cdb2589ea15554 9497a4ee62e1
  0 69982ec78c6dd2f24b3b62f3e2baaa79ab48ed93 69982ec78c6d 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 7eeab2ea75ec
  $ mv .hg/git-mapfile-bak .hg/git-mapfile
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

Try other extensioins that provide "{gitnode}":
  $ hg log -r 'tip^' --template "{gitnode}\n"
  7e2a5465ff4e3b992c429bb87a392620a0ac97b7
  $ hg log -r 'tip^' --template "{gitnode}\n" --config extensions.fbscmquery=
  7e2a5465ff4e3b992c429bb87a392620a0ac97b7
  $ hg log -r 'tip^' --template "{gitnode}\n" --config extensions.gitrevset=
  7e2a5465ff4e3b992c429bb87a392620a0ac97b7
