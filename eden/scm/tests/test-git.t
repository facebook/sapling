#chg-compatible
#require git no-windows

  $ export GIT_AUTHOR_NAME='test'
  $ export GIT_AUTHOR_EMAIL='test@example.org'
  $ export GIT_AUTHOR_DATE="2007-01-01 00:00:10 +0000"
  $ export GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"
  $ export GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"
  $ export GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  $ setconfig diff.git=true ui.allowemptycommit=true
  $ unset GIT_DIR

Prepare a git repo:

  $ git init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha

  $ echo 2 > beta
  $ git add beta
  $ git commit -q -mbeta

Init an hg repo using the git changelog backend:

  $ cd $TESTTMP
  $ hg debuginitgit --git-dir gitrepo/.git repo1
  $ cd repo1

  $ hg log -Gr 'all()' -T '{node} {desc}'
  o  3f5848713286c67b8a71a450e98c7fa66787bde2 beta
  │
  o  b6c31add3e60ded7a9c9c803641edffb1dccd251 alpha
  
  $ hg debugchangelog
  The changelog is backed by Rust. More backend information:
  Backend (segmented git):
    Local:
      Segments + IdMap: $TESTTMP/repo1/.hg/store/segments/v1
      Git: $TESTTMP/gitrepo/.git
  Feature Providers:
    Commit Graph Algorithms:
      Segments
    Commit Hash / Rev Lookup:
      IdMap
    Commit Data (user, message):
      Git

Test checkout:

  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo *
  alpha beta
  $ cat beta
  2

Test diff:

  $ hg log -r tip -p
  commit:      3f5848713286
  bookmark:    master
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:10 2007 +0000
  summary:     beta
  
  diff --git a/beta b/beta
  new file mode 100644
  --- /dev/null
  +++ b/beta
  @@ -0,0 +1,1 @@
  +2
  
Test status:

  $ hg status
  $ echo 3 > alpha
  $ hg status
  M alpha

Test commit:

  $ hg commit -m alpha3
  $ hg log -Gr: -T '{desc}'
  @  alpha3
  │
  o  beta
  │
  o  alpha
  
Test log FILE:

  $ hg log -G -T '{desc}' alpha
  @  alpha3
  ╷
  o  alpha
  
Test log FILE with patches:

  $ hg log -p -G -T '{desc}\n' alpha
  @  alpha3
  ╷  diff --git a/alpha b/alpha
  ╷  --- a/alpha
  ╷  +++ b/alpha
  ╷  @@ -1,1 +1,1 @@
  ╷  -1
  ╷  +3
  ╷
  o  alpha
     diff --git a/alpha b/alpha
     new file mode 100644
     --- /dev/null
     +++ b/alpha
     @@ -0,0 +1,1 @@
     +1
  

Test bookmarks:

  $ hg bookmark -r. foo
  $ hg bookmarks
     foo                       5c9a5ee451a8
     master                    3f5848713286

Test changes are readable via git:

  $ export GIT_DIR="$TESTTMP/gitrepo/.git"
  $ git log foo --pretty='format:%s %an %d'
  alpha3 test  (refs/visibleheads/5c9a5ee451a8051f0d16433dee8a2c2259d5fed8, foo)
  beta test  (HEAD -> master)
  alpha test  (no-eol)

Exercise pathcopies code path:

  $ hg diff -r '.^^' -r .
  diff --git a/alpha b/alpha
  --- a/alpha
  +++ b/alpha
  @@ -1,1 +1,1 @@
  -1
  +3
  diff --git a/beta b/beta
  new file mode 100644
  --- /dev/null
  +++ b/beta
  @@ -0,0 +1,1 @@
  +2

Prepare a new git "client" repo:

  $ unset GIT_DIR
  $ git init -q --bare $TESTTMP/gitrepo2
  $ cd "$TESTTMP/gitrepo2"
  $ git remote add origin "$TESTTMP/gitrepo/.git"
  $ hg debuginitgit --git-dir="$TESTTMP/gitrepo2" "$TESTTMP/repo2"
  $ cd "$TESTTMP/repo2"

Test pull:

- pull with -B
  $ hg pull -B foo
  From $TESTTMP/gitrepo/
   * branch            foo        -> FETCH_HEAD
   * [new branch]      foo        -> origin/foo
  $ hg log -r origin/foo -T '{desc}\n'
  alpha3

- pull with -B and --update
  $ hg pull -q origin -B master --update
  $ hg log -r . -T '{remotenames}\n'
  origin/master

- pull without arguments
  $ hg pull

- infinitepush compatibility
  $ hg pull --config extensions.infinitepush=

Test clone with flags (--noupdate, --updaterev):

  $ mkdir $TESTTMP/clonetest
  $ cd $TESTTMP/clonetest

  $ hg clone -q --noupdate "git+file://$TESTTMP/gitrepo"
  $ cd gitrepo
  $ hg log -r . -T '{node|short}\n'
  000000000000
  $ hg bookmarks --remote
     origin/foo                5c9a5ee451a8
     origin/master             3f5848713286
  $ cd ..

  $ hg clone "git+file://$TESTTMP/gitrepo" cloned1
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new branch]      foo        -> origin/foo
   * [new branch]      master     -> origin/master
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd cloned1 log -r . -T '{node|short} {remotenames} {desc}\n'
  5c9a5ee451a8 origin/foo alpha3
  $ cd ..

  $ hg clone --updaterev origin/foo "git+file://$TESTTMP/gitrepo" cloned2
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new branch]      foo        -> origin/foo
   * [new branch]      master     -> origin/master
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd cloned2 log -r . -T '{node|short} {remotenames} {desc}\n'
  5c9a5ee451a8 origin/foo alpha3
  $ cd ..

Test push:

  $ cd "$TESTTMP/clonetest/cloned1"
  $ echo 3 > beta
  $ hg commit -m 'beta.change'

- --to without -r
  $ hg push -q --to book_change_beta

- --to with -r
  $ hg push -r '.^' --to parent_change_beta
  To file:/*/$TESTTMP/gitrepo (glob)
   * [new branch]      5c9a5ee451a8051f0d16433dee8a2c2259d5fed8 -> parent_change_beta

  $ hg log -r '.^+.' -T '{desc} {remotenames}\n'
  alpha3 origin/foo origin/parent_change_beta
  beta.change origin/book_change_beta

- delete bookmark
  $ hg push --delete book_change_beta
  To file:/*/$TESTTMP/gitrepo (glob)
   - [deleted]         book_change_beta

  $ hg log -r '.^+.' -T '{desc} {remotenames}\n'
  alpha3 origin/foo origin/parent_change_beta
  beta.change 

- infinitepush compatibility
  $ hg push -q -r '.^' --to push_with_infinitepush --config extensions.infinitepush=

- push with --force

  $ cd "$TESTTMP"
  $ git init -qb main --bare "pushforce.git"
  $ hg clone "git+file://$TESTTMP/pushforce.git"
  $ cd pushforce
  $ git --git-dir=.hg/store/git config advice.pushUpdateRejected false

  $ drawdag << 'EOS'
  > B C
  > |/
  > A
  > EOS

  $ hg push -qr $B --to foo
  $ hg push -qr $C --to foo
  To file:/*/$TESTTMP/pushforce.git (glob)
   ! [rejected]        5d38a953d58b0c80a4416ba62e62d3f2985a3726 -> foo (non-fast-forward)
  error: failed to push some refs to 'file:/*/$TESTTMP/pushforce.git' (glob)
  [1]
  $ hg push -qr $C --to foo --force

- push without --to

  $ cd "$TESTTMP"
  $ git init -qb main --bare "pushto.git"
  $ hg clone "git+file://$TESTTMP/pushto.git"
  $ cd pushto

  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ hg push -qr $A --to stable
  $ hg push -qr $B --to main
  $ hg up -q $B
  $ hg commit -m C

 (pick "main" automatically)
  $ hg push
  To file:/*/$TESTTMP/pushto.git (glob)
     0de3093..a9d5bd6  a9d5bd6ac8bcf89de9cd99fd215cca243e8aeed9 -> main
  $ hg push -q --to stable

 (cannot pick with multiple candidates)
  $ hg commit -m D
  $ hg push
  abort: use '--to' to specify destination bookmark
  [255]

"files" metadata:

  $ hg log -r $A+$B -T '{files}\n'
  A
  B

Submodule does not cause a crash:

  $ cd
  $ git init -q submod
  $ cd submod

  $ git submodule --quiet add ../gitrepo b
  $ echo 1 > a
  $ echo 2 > c
  $ git add a c
  $ git commit --quiet -m s

- checkout silently ignores the submodule

  $ cd
  $ hg clone "git+file://$TESTTMP/submod" cloned-submod
  From file:/*/$TESTTMP/submod (glob)
   * [new branch]      master     -> origin/master
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd cloned-submod
  $ echo *
  a c

- changing the tree does not lose submodule

  $ touch d
  $ hg commit -m d -A d
  $ hg book changed
  $ git --git-dir=.hg/store/git cat-file -p changed:
  100644 blob 703feeadc77c10eeec4dfe76ae58506b6a77ab11	.gitmodules
  100644 blob d00491fd7e5bb6fa28c517a0bb32b8b506539d4d	a
  160000 commit 3f5848713286c67b8a71a450e98c7fa66787bde2	b
  100644 blob 0cfbf08886fca9a91cb753ec8734c84fcbe52c9f	c
  100644 blob e69de29bb2d1d6434b8b29ae775ad8c2e48c5391	d

Tags are ignored during clone and pull:

  $ cd
  $ git init -b main -q gittag
  $ cd gittag
  $ echo 1 > a
  $ git add a
  $ git commit -q -m a
  $ git tag v1

  $ cd
  $ hg clone -q git+file://$TESTTMP/gittag cloned-gittag
  $ cd cloned-gittag
  $ hg pull
  $ hg bookmarks
  no bookmarks set
  $ hg bookmarks --remote
     origin/main               379d702a285c
  $ git --git-dir=.hg/store/git for-each-ref
  379d702a285c1e34e6365cc347249ec73bcd6b40 commit	refs/remotes/origin/main
