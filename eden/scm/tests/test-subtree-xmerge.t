#require git no-windows

  $ setconfig diff.git=True
  $ setconfig subtree.allow-any-source-commit=True
  $ setconfig subtree.min-path-depth=1

prepare a git repo:

  $ . $TESTDIR/git.sh
  $ git -c init.defaultBranch=main init -q gitrepo
  $ cd gitrepo
  $ git config core.autocrlf false
  $ echo 1 > alpha
  $ git add alpha
  $ git commit -q -malpha

  $ mkdir dir1
  $ echo "b1\nb2\nb3" > dir1/beta
  $ git add dir1/beta
  $ git commit -q -mbeta

  $ echo "b1\nb2\nb33" > dir1/beta
  $ git add dir1/beta
  $ git commit -q -m "update beta"

  $ mkdir dir2
  $ echo "g1\ng2\ng3" > dir2/gamma
  $ git add dir2/gamma
  $ git commit -q -mgamma

  $ echo "g1\ng2\ng33" > dir2/gamma
  $ git add dir2/gamma
  $ git commit -q -m "update gamma"

  $ git log --graph
  * commit 421b13f5014cb2fdb2782fbea8de256066f975c8
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     update gamma
  | 
  * commit 7d1aff4267dd226a9f5550790c2bdd89c8ff2c59
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     gamma
  | 
  * commit b25d15e29c29a8deb2bf55184109c4fb6913bcea
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     update beta
  | 
  * commit 1c8131597324d8fbbdbbdae1e8a48d18559dd303
  | Author: test <test@example.org>
  | Date:   Mon Jan 1 00:00:10 2007 +0000
  | 
  |     beta
  | 
  * commit b6c31add3e60ded7a9c9c803641edffb1dccd251
    Author: test <test@example.org>
    Date:   Mon Jan 1 00:00:10 2007 +0000
    
        alpha

  $ export GIT_URL=git+file://$TESTTMP/gitrepo

Prepare a Sapling repo:

  $ newclientrepo
  $ drawdag <<'EOS'
  > B   # B/foo/y = bbb\n
  > |
  > A   # A/foo/x = aaa\n
  >     # drawdag.defaultfiles=false
  > EOS
  $ hg go $B -q

  $ hg subtree import --url $GIT_URL --rev 1c8131597324d8fbbdbbdae1e8a48d18559dd303 --to-path bar -m "import gitrepo to bar"
  creating git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  From file:/*/$TESTTMP/gitrepo (glob)
   * [new ref]         421b13f5014cb2fdb2782fbea8de256066f975c8 -> remote/main
   * [new ref]         1c8131597324d8fbbdbbdae1e8a48d18559dd303 -> refs/visibleheads/1c8131597324d8fbbdbbdae1e8a48d18559dd303
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  copying / to bar

  $ echo "b1\nb2\nb3~" > bar/dir1/beta
  $ hg ci -m "update bar/dir1/beta"


Cross-repo subtree merge doesn't support merge-base-strategy option

  $ hg subtree merge --url $GIT_URL --rev b25d15e29c29a8deb2bf55184109c4fb6913bcea --from-path "" --to-path bar --merge-base-strategy --only-to
  abort: cannot specify both url and merge-base-strategy
  [255]

Subtree merge should fail with conflicts:

  $ hg subtree merge --url $GIT_URL --rev b25d15e29c29a8deb2bf55184109c4fb6913bcea --from-path "" --to-path bar
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  searching for merge base ...
  found the last subtree import commit * (glob)
  merge base: 1c8131597324
  merging bar/dir1/beta and dir1/beta to bar/dir1/beta
  warning: 1 conflicts while merging bar/dir1/beta! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg diff
  diff --git a/bar/dir1/beta b/bar/dir1/beta
  --- a/bar/dir1/beta
  +++ b/bar/dir1/beta
  @@ -1,3 +1,7 @@
   b1
   b2
  +<<<<<<< working copy: * - test: update bar/dir1/beta (glob)
   b3~
  +=======
  +b33
  +>>>>>>> merge rev:    b25d15e29c29 - test: update beta
  $ echo "b1\nb2\nb33~" > bar/dir1/beta
  $ hg resolve --mark bar/dir1/beta
  (no more unresolved files)
  $ hg ci -m "merge gitrepo to bar"
  $ hg subtree inspect -r .
  {
    "xmerges": [
      {
        "version": 1,
        "url": "file:/*/$TESTTMP/gitrepo", (glob)
        "from_commit": "b25d15e29c29a8deb2bf55184109c4fb6913bcea",
        "from_path": "",
        "to_path": "bar"
      }
    ]
  }

Subtree merge should succeed without conflicts:

  $ hg subtree merge --url $GIT_URL --rev 421b13f5014cb2fdb2782fbea8de256066f975c8 --from-path "" --to-path bar
  using cached git repo at $TESTTMP/default-hgcache/gitrepos/* (glob)
  searching for merge base ...
  found the last subtree cross-repo merge commit * (glob)
  merge base: b25d15e29c29
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (subtree merge, don't forget to commit)
  $ hg ci -m "merge gitrepo to bar again"
  $ hg show
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/dir2/gamma
  description:
  merge gitrepo to bar again
  
  Subtree merge from 421b13f5014cb2fdb2782fbea8de256066f975c8
  - Merged path  to bar
  
  
  diff --git a/bar/dir2/gamma b/bar/dir2/gamma
  new file mode 100644
  --- /dev/null
  +++ b/bar/dir2/gamma
  @@ -0,0 +1,3 @@
  +g1
  +g2
  +g33
  $ hg subtree inspect -r .
  {
    "xmerges": [
      {
        "version": 1,
        "url": "file:/*/$TESTTMP/gitrepo", (glob)
        "from_commit": "421b13f5014cb2fdb2782fbea8de256066f975c8",
        "from_path": "",
        "to_path": "bar"
      }
    ]
  }
