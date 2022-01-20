#chg-compatible
#require git no-windows no-fsmonitor

  $ setconfig diff.git=true ui.allowemptycommit=true

Prepare smaller submodules

  $ hg init --git sub1
  $ drawdag --cwd sub1 << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg bookmark --cwd sub1 -r $B main 

  $ hg init --git sub2
  $ drawdag --cwd sub2 << 'EOS'
  > D 
  > |
  > C 
  > EOS
  $ hg bookmark --cwd sub2 -r $D main 

Prepare git repo with submodules
(commit hashes are unstable because '.gitmodules' contains TESTTMP paths)

  $ git init -q -b main parent-repo-git
  $ cd parent-repo-git
  $ git submodule --quiet add -b main file://$TESTTMP/sub1/.hg/store/git mod/1
  $ git submodule --quiet add -b main file://$TESTTMP/sub2/.hg/store/git mod/2
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
  $ hg clone git+file://$TESTTMP/parent-repo-git parent-repo-hg
  From file:/*/$TESTTMP/parent-repo-git (glob)
   * [new branch]      main       -> origin/main
  pulling submodule mod/1
  pulling submodule mod/2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd parent-repo-hg
  $ echo mod/*/*
  mod/1/A mod/2/C

Checking out commits triggers submodule updates

  $ hg checkout '.^'
  pulling submodule mod/1
  pulling submodule mod/2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo mod/*/*
  mod/1/A mod/1/B mod/2/C mod/2/D

  $ hg checkout main
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo mod/*/*
  mod/1/A mod/2/C

Make changes to submodules via working copy

  $ hg --cwd mod/1 up -q $B
  $ hg status
  M mod/1

  $ hg --cwd mod/2 up -q null
  $ hg status
  M mod/1
  R mod/2

  $ hg status mod/1
  M mod/1

  $ hg status mod/2
  R mod/2

  $ rm -rf mod/2
  $ hg status
  M mod/1
  R mod/2

  $ cat >> .gitmodules << EOF
  > [submodule "sub3"]
  > url = file://$TESTTMP/sub1/.hg/store/git
  > path = mod/3
  > EOF

  $ hg status
  M .gitmodules
  M mod/1
  R mod/2

  $ hg clone -q git+file://$TESTTMP/sub1/.hg/store/git mod/3
  $ hg status
  M .gitmodules
  M mod/1
  A mod/3
  R mod/2

Commit submodule changes

  $ hg commit -m 'submodule change with file patterns' mod/1
  $ hg status
  M .gitmodules
  A mod/3
  R mod/2

  $ hg commit -m 'submodule change without file patterns'
  $ hg status

  $ echo mod/*/*
  mod/1/A mod/1/B mod/3/A mod/3/B
  $ hg push -q --to foo --create

Try checking out the submodule change made by hg

  $ cd
  $ hg clone -qU git+file://$TESTTMP/parent-repo-git parent-repo-hg2
  $ cd parent-repo-hg2
  $ hg pull -B foo --update
  From file:/*/$TESTTMP/parent-repo-git (glob)
   * [new branch]      foo        -> origin/foo
  pulling submodule mod/1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo mod/*/*
  mod/1/A mod/1/B mod/3/A mod/3/B

  $ hg up -q '.^'
  $ echo mod/*/*
  mod/1/A mod/1/B mod/2/C mod/3/A mod/3/B

Nested submodules can share submodules with same URLs

  $ cd
  $ git init -q -b main grandparent-repo-git
  $ cd grandparent-repo-git
  $ git submodule --quiet add -b main file://$TESTTMP/sub1/.hg/store/git mod/1
  $ git submodule --quiet add -b main file://$TESTTMP/parent-repo-git/.git mod/p
  $ git commit -qm 'add .gitmodules'

  $ cd
  $ hg clone git+file://$TESTTMP/grandparent-repo-git grandparent-repo-hg
  From file:/*/$TESTTMP/grandparent-repo-git (glob)
   * [new branch]      main       -> origin/main
  pulling submodule mod/1
  pulling submodule mod/p
  pulling submodule mod/p/mod/2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd grandparent-repo-hg
  $ echo mod/* mod/*/mod/*
  mod/1 mod/p mod/p/mod/1 mod/p/mod/2

  $ cd .hg/store/gitmodules
  $ find | grep gitmodules
  [1]
