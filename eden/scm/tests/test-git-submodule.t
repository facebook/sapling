#require git no-windows no-eden
#modern-config-incompatible

Make sure things still work for submodules not properly namespaced (fixed in D63775983).
#testcases normal submodule-namespace-bug


  $ export HGIDENTITY=sl
  $ . $TESTDIR/git.sh
  $ setconfig diff.git=true ui.allowemptycommit=true
  $ enable rebase

Prepare smaller submodules

  $ sl init --git sub1
  $ drawdag --cwd sub1 << 'EOS'
  > B
  > |
  > A
  > EOS
  $ sl bookmark --cwd sub1 -r $B main

  $ sl init --git sub2
  $ drawdag --cwd sub2 << 'EOS'
  > D
  > |
  > C
  > EOS
  $ sl bookmark --cwd sub2 -r $D main

Prepare git repo with submodules
(commit hashes are unstable because '.gitmodules' contains TESTTMP paths)

  $ git init -q -b main parent-repo-git
  $ cd parent-repo-git
  $ git submodule --quiet add -b main file://$TESTTMP/sub1/.sl/store/git mod/1
  $ git submodule --quiet add -b main file://$TESTTMP/sub2/.sl/store/git mod/2
  $ git commit -qm 'add .gitmodules'

  $ cd mod/1
  $ git checkout -q 'HEAD^'
  $ cd ../2
  $ git checkout -q 'HEAD^'
  $ cd ../..
  $ git commit -am 'checkout older submodule commits'
  [main *] checkout older submodule commits (glob)
   2 files changed, 2 insertions(+), 2 deletions(-)

Clone the git repo with submodules

  $ cd

#if submodule-namespace-bug
Reproduce the lack of submodule namespacing:

  $ cat > namespace_bug.py <<EOF
  > def badjoin(self, path, *paths):
  >   return self.inner.join(path)
  > from sapling import store
  > store.metavfs.join = badjoin
  > EOF

  $ sl clone -q --git "$TESTTMP/parent-repo-git" parent-repo-hg --config extensions.namespacebug=namespace_bug.py
#else
  $ sl clone --git "$TESTTMP/parent-repo-git" parent-repo-hg
  From $TESTTMP/parent-repo-git
   * [new ref]         * -> remote/main (glob)
  pulling submodule mod/1
  pulling submodule mod/2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
#endif

  $ cd parent-repo-hg
  $ echo mod/*/*
  mod/1/A mod/2/C

subdmodules are namespaced into separate directories
#if submodule-namespace-bug
  $ ls .sl/store/gitmodules | wc -l
  0
#else
  $ ls .sl/store/gitmodules | wc -l
  2
#endif

Checking out commits triggers submodule updates

  $ sl checkout '.^' --config experimental.submodule-pull-quiet=False
  pulling submodule mod/1
  From * (glob)
     73c8ee0..0de3093  0de30934572f96ff6d3cbfc70aa8b46ef95dbb42 -> parent/mod_1
  pulling submodule mod/2
  From * (glob)
     f4140cb..f02e91c  f02e91cd72c210709673488ad9224fdc72e49018 -> parent/mod_2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo mod/*/*
  mod/1/A mod/1/B mod/2/C mod/2/D

  $ LOG=ext::sigtrace=debug sl checkout main --config extensions.sigtrace= --config sigtrace.interval=60
  DEBUG ext::sigtrace: starting sigtrace thread (?)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo mod/*/*
  mod/1/A mod/2/C

Don't crash on "sl manifest":
  $ sl manifest -v
  644   .gitmodules
   submod mod/1
   submod mod/2

Make changes to submodules via working copy

  $ sl --cwd mod/1 up -q $B
  $ sl status
  M mod/1

  $ sl --cwd mod/2 up -q null
  $ sl status
  M mod/1
  R mod/2

  $ sl status mod/1
  M mod/1

  $ sl status mod/2
  R mod/2

  $ rm -rf mod/2
  $ sl status
  M mod/1
  R mod/2

  $ cat >> .gitmodules << EOF
  > [submodule "sub3"]
  > url = file://$TESTTMP/sub1/.sl/store/git
  > path = mod/3
  > EOF

  $ sl status
  M .gitmodules
  M mod/1
  R mod/2

  $ sl clone -q --git "$TESTTMP/sub1/.sl/store/git" mod/3
  $ sl status
  M .gitmodules
  M mod/1
  A mod/3
  R mod/2

Diff working copy changes

  $ sl diff mod
  diff --git a/mod/1 b/mod/1
  --- a/mod/1
  +++ b/mod/1
  @@ -1,1 +1,1 @@
  -Subproject commit 73c8ee0cae8ffb843cc154c3bf28a12438801d3f
  +Subproject commit 0de30934572f96ff6d3cbfc70aa8b46ef95dbb42
  diff --git a/mod/2 b/mod/2
  deleted file mode 160000
  --- a/mod/2
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -Subproject commit f4140cb61bcd309e2a17e95f50ae419c7729a6bc
  diff --git a/mod/3 b/mod/3
  new file mode 160000
  --- /dev/null
  +++ b/mod/3
  @@ -0,0 +1,1 @@
  +Subproject commit 0de30934572f96ff6d3cbfc70aa8b46ef95dbb42

Commit submodule changes

  $ sl commit -m 'submodule change with file patterns' mod/1
  $ sl status
  M .gitmodules
  A mod/3
  R mod/2

  $ sl commit -m 'submodule change without file patterns'
  $ sl status

  $ echo mod/*/*
  mod/1/A mod/1/B mod/3/A mod/3/B
  $ sl push -q --to foo --create

Diff committed changes

  $ sl diff -r '.^^' -r .
  diff --git a/.gitmodules b/.gitmodules
  --- a/.gitmodules
  +++ b/.gitmodules
  @@ -6,3 +6,6 @@
   	path = mod/2
   	url = file:/*/$TESTTMP/sub2/.sl/store/git (glob)
   	branch = main
  +[submodule "sub3"]
  +url = file:/*/$TESTTMP/sub1/.sl/store/git (glob)
  +path = mod/3
  diff --git a/mod/1 b/mod/1
  --- a/mod/1
  +++ b/mod/1
  @@ -1,1 +1,1 @@
  -Subproject commit 73c8ee0cae8ffb843cc154c3bf28a12438801d3f
  +Subproject commit 0de30934572f96ff6d3cbfc70aa8b46ef95dbb42
  diff --git a/mod/2 b/mod/2
  deleted file mode 160000
  --- a/mod/2
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -Subproject commit f4140cb61bcd309e2a17e95f50ae419c7729a6bc
  diff --git a/mod/3 b/mod/3
  new file mode 160000
  --- /dev/null
  +++ b/mod/3
  @@ -0,0 +1,1 @@
  +Subproject commit 0de30934572f96ff6d3cbfc70aa8b46ef95dbb42

Try checking out the submodule change made by hg

  $ cd
  $ sl clone -qU --git "$TESTTMP/parent-repo-git" parent-repo-hg2
  $ cd parent-repo-hg2
  $ sl pull -B foo --update
  pulling from $TESTTMP/parent-repo-git
  From $TESTTMP/parent-repo-git
   * [new ref]         * -> remote/foo (glob)
  pulling submodule mod/1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo mod/*/*
  mod/1/A mod/1/B mod/3/A mod/3/B

  $ sl up -q '.^'
  $ echo mod/*/*
  mod/1/A mod/1/B mod/2/C mod/3/A mod/3/B

Nested submodules can share submodules with same URLs

  $ cd
  $ git init -q -b main grandparent-repo-git
  $ cd grandparent-repo-git
  $ git submodule --quiet add -b main file://$TESTTMP/sub1/.sl/store/git mod/1
  $ git submodule --quiet add -b main file://$TESTTMP/parent-repo-git/.git mod/p
  $ git commit -qm 'add .gitmodules'

  $ cd
  $ sl clone --git "$TESTTMP/grandparent-repo-git" grandparent-repo-hg
  From $TESTTMP/grandparent-repo-git
   * [new ref]         * -> remote/main (glob)
  pulling submodule mod/1
  pulling submodule mod/p
  pulling submodule mod/p/mod/2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd grandparent-repo-hg
  $ echo mod/* mod/*/mod/*
  mod/1 mod/p mod/p/mod/1 mod/p/mod/2

  $ cd .sl/store/gitmodules
  $ find . | grep gitmodules
  [1]

Rebase submodule change

  $ cd
  $ git init -q -b main rebase-git
  $ cd rebase-git
  $ git submodule --quiet add -b main file://$TESTTMP/sub1/.sl/store/git m1
  $ git submodule --quiet add -b main file://$TESTTMP/sub2/.sl/store/git m2
  $ git commit -qm A

  $ cd
  $ sl clone -q --git "$TESTTMP/rebase-git" rebase-hg
  $ cd rebase-hg
  $ touch B
  $ sl commit -Aqm B B
  $ sl --cwd m2 checkout -q '.^'
  $ sl commit -qm C

  $ sl rebase -r . -d 'desc(A)' --config rebase.experimental.inmemory=false
  rebasing * "C" (glob)
  $ sl log -r '.' -p -T '{desc}\n'
  C
  diff --git a/m2 b/m2
  --- a/m2
  +++ b/m2
  @@ -1,1 +1,1 @@
  -Subproject commit f02e91cd72c210709673488ad9224fdc72e49018
  +Subproject commit f4140cb61bcd309e2a17e95f50ae419c7729a6bc
  
  $ sl st
  $ echo m2/*
  m2/C

  $ sl rebase -r . -d 'desc(B)' --config rebase.experimental.inmemory=true
  rebasing * "C" (glob)
  $ sl log -r '.' -p -T '{desc}\n'
  C
  diff --git a/m2 b/m2
  --- a/m2
  +++ b/m2
  @@ -1,1 +1,1 @@
  -Subproject commit f02e91cd72c210709673488ad9224fdc72e49018
  +Subproject commit f4140cb61bcd309e2a17e95f50ae419c7729a6bc
  $ sl st
  $ echo m2/*
  m2/C

Goto keeps checking out the other submodules after one fails

  $ cd
  $ sl clone -q --git "$TESTTMP/parent-repo-git" keepgoing-hg
  $ cd keepgoing-hg
  $ echo dirty > mod/1/B
  $ sl goto '.^'
  pulling submodule mod/1
  B: untracked file differs
  pulling submodule mod/2
  abort: could not check out 1 project
    mod/1 (0de30934572f96ff6d3cbfc70aa8b46ef95dbb42):
      untracked files in working directory differ from files in requested revision
  (if local changes conflict, commit, stash or remove them and retry, or use 'sl goto --clean' to discard them)
  [255]
  $ echo mod/*/*
  mod/1/A mod/1/B mod/2/C mod/2/D
  $ cat mod/1/B
  dirty
