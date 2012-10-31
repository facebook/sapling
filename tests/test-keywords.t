Load commonly used test logic
  $ . "$TESTDIR/testutil"

bail if the user does not have git command-line client
  $ "$TESTDIR/hghave" git || exit 80

bail if the user does not have dulwich
  $ python -c 'import dulwich, dulwich.repo' || exit 80

  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE

  $ count=10
  $ commit()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$count +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error"
  >     count=`expr $count + 1`
  > }

  $ mkdir gitrepo
  $ cd gitrepo
  $ git init
  Initialized empty Git repository in $TESTTMP/gitrepo/.git/
  $ echo alpha > alpha
  $ git add alpha
  $ commit -m 'add alpha'
  $ echo beta > beta
  $ git add beta
  $ commit -m 'add beta'

  $ cd ..

  $ hg clone gitrepo hgrepo | grep -v '^updating'
  importing git objects into hg
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd hgrepo
  $ echo gamma > gamma
  $ hg add gamma
  $ hg commit -m 'add gamma'

  $ hg log --template "{rev} {node} {node|short} {gitnode} {gitnode|short}\n"
  2 a9da0c7c9bb7574b0f3139ab65cabac7468d6b8d a9da0c7c9bb7  
  1 7bcd915dc873c654b822f01b0a39269b2739e86d 7bcd915dc873 9497a4ee62e16ee641860d7677cdb2589ea15554 9497a4ee62e1
  0 3442585be8a60c6cd476bbc4e45755339f2a23ef 3442585be8a6 7eeab2ea75ec1ac0ff3d500b5b6f8a3447dd7c03 7eeab2ea75ec
  $ hg log --template "fromgit {rev}\n" --rev "fromgit()"
  fromgit 0
  fromgit 1
  $ hg log --template "gitnode_existsA {rev}\n" --rev "gitnode(9497a4ee62e16ee641860d7677cdb2589ea15554)"
  gitnode_existsA 1
  $ hg log --template "gitnode_existsB {rev}\n" --rev "gitnode(7eeab2ea75ec)"
  gitnode_existsB 0
  $ hg log --template "gitnode_notexists {rev}\n" --rev "gitnode(1234567890ab)"
