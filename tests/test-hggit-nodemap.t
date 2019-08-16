Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

set up a git repo
  $ git init -q gitrepo
  $ cd gitrepo
  $ echo alpha > alpha
  $ git add alpha
  $ fn_git_commit -m 'add alpha'
  $ git checkout -qb beta
  $ echo beta > beta
  $ git add beta
  $ fn_git_commit -m 'add beta'
  $ cd ..

pull a branch with the old mapfile
  $ hg init hgrepo
  $ cd hgrepo
  $ echo "[paths]" >> .hg/hgrc
  $ echo "default=$TESTTMP/gitrepo" >> .hg/hgrc
  $ hg pull -r master
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  $ ls -d .hg/git-mapfile*
  .hg/git-mapfile
  $ hg log -r tip -T '{gitnode}\n'
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03

pull more commits with the new nodemap
  $ setconfig hggit.indexedlognodemap=True
  $ hg pull -r beta
  pulling from $TESTTMP/gitrepo
  importing git objects into hg
  $ ls -d .hg/git-mapfile*
  .hg/git-mapfile
  .hg/git-mapfile-log
  $ hg log -r 'tip^::tip' -T '{gitnode}\n'
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  9497a4ee62e16ee641860d7677cdb2589ea15554

can still get the mapping without the old map file
  $ mv .hg/git-mapfile .hg/git-mapfile.old
  $ hg log -r 'tip^::tip' -T '{gitnode}\n'
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  9497a4ee62e16ee641860d7677cdb2589ea15554
  $ mv .hg/git-mapfile.old .hg/git-mapfile

can still get the mapping without the nodemap
  $ mv .hg/git-mapfile-log .hg/git-mapfile-log.old
  $ hg log -r 'tip^::tip' -T '{gitnode}\n'
  7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03
  9497a4ee62e16ee641860d7677cdb2589ea15554
  $ mv .hg/git-mapfile-log.old .hg/git-mapfile-log

git cleanup cleans nodemap
  $ hg bundle -r tip --base 'tip^' ../mybundle.hg
  1 changesets found
  $ hg debugstrip -r tip --no-backup
  $ hg git-cleanup
  git commit map cleaned
  $ hg unbundle -q ../mybundle.hg
  $ hg log -r tip -T '{gitnode}\n'
  9497a4ee62e16ee641860d7677cdb2589ea15554
