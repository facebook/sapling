#chg-compatible
#require git no-windows
#debugruntest-compatible

  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true

Prepare git repo with tags

  $ git init -b main git-repo
  Initialized empty Git repository in $TESTTMP/git-repo/.git/
  $ cd git-repo
  $ touch a
  $ git add a
  $ git commit -qm A
  $ git tag v1
  $ echo 1 >> a
  $ git commit -qam B
  $ git tag v2
  $ git -c advice.detachHead=false checkout -q 'HEAD^'
  $ git branch -f main

Clone it

  $ cd
  $ hg clone -q --git $TESTTMP/git-repo hg-repo
  $ cd hg-repo

No remotenames about tags initially

  $ hg log -Gr: -T '{remotenames} {desc}'
  @  remote/main A
  
Pull tags explicitly

  $ hg pull -B tags/v1
  pulling from $TESTTMP/git-repo
  From $TESTTMP/git-repo
   * [new ref]         bfff4215bb0ba84b76577621c9974de957610ecb -> remote/tags/v1

Pull implicitly via autopull

  $ hg goto tags/v2
  pulling 'tags/v2' from '$TESTTMP/git-repo'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Verify the pulled tags can be seen

  $ hg log -Gr: -T '{remotenames} {desc}'
  @  remote/tags/v2 B
  │
  o  remote/main remote/tags/v1 A
  
Pulled tags are listed as remote names

  $ hg bookmarks --remote
     remote/main               bfff4215bb0b
     remote/tags/v1            bfff4215bb0b
     remote/tags/v2            e8a8a5525653

Push tags

  $ echo 2 > a
  $ hg commit -m C
  $ hg push --to tags/v3
  To $TESTTMP/git-repo
   * [new tag]         42d0e8258ed6249380f83aaf4564a0c0865ae5f7 -> v3
  $ hg log -Gr: -T '{remotenames} {desc}'
  @  remote/tags/v3 C
  │
  o  remote/tags/v2 B
  │
  o  remote/main remote/tags/v1 A
  
Verify the pushed tag can be seen by git

  $ GIT_DIR="$TESTTMP/git-repo/.git" git for-each-ref
  bfff4215bb0ba84b76577621c9974de957610ecb commit	refs/heads/main
  bfff4215bb0ba84b76577621c9974de957610ecb commit	refs/tags/v1
  e8a8a552565346d086e22288b8cf16ef2cb2267e commit	refs/tags/v2
  42d0e8258ed6249380f83aaf4564a0c0865ae5f7 commit	refs/tags/v3
