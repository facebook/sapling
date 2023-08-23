#debugruntest-compatible
#inprocess-hg-incompatible
#require git no-windows

  $ eagerepo
  $ setconfig ghstack.github_username=test clone.use-rust=false
Threading messes up the asyncio run loop since only main thread gets run loop by default.
  $ enable github ghstack amend rebase smartlog
  $ . $TESTDIR/git.sh
  $ export SL_TEST_GH_URL=https://github.com/facebook/test_github_repo.git

"upstream" represents the remote repo

  $ git init upstream -qb main
  $ cd upstream
  $ touch a
  $ git add a
  $ git commit -aqm foo
Switch to non-main branch otherwise "push" from client repo fails.
  $ git checkout -qb some-other-branch


"client" represents the local clone

  $ cd
  $ sl clone --git -q file://$TESTTMP/upstream client -u main
  $ cd client
  $ sl debugmakepublic main

Hook in all our mocks and stubs to make this test work.
  $ setconfig extensions.mock_ghstack_land=$TESTDIR/github/mock_ghstack_land.py

Create a single commit stack.
  $ touch b
  $ sl commit -Aqm b
  $ sl ghstack submit --short
  To file:/*/$TESTTMP/upstream (glob)
   * [new branch]      3c2e2c027c0785e0926f51252735a374eff57f51 -> gh/test/0/head
   * [new branch]      986973ee4344290d90daacf228b74c62bda520f8 -> gh/test/0/base
  To file:/*/$TESTTMP/upstream (glob)
   * [new branch]      6587f91ed4d9ef5e974ecf588a7a4b7ba99c6af2 -> gh/test/0/orig
  https://github.com/facebook/test_github_repo/pull/1

  $ sl smartlog -T '{node|short} {desc|firstline} {github_pull_request_number}'
  @  6587f91ed4d9 b 1
  │
  o  986973ee4344 foo
  

  $ sl ghstack land https://github.com/facebook/test_github_repo/pull/1
  pulling from file:/*/$TESTTMP/upstream (glob)
  To file:/*/$TESTTMP/upstream (glob)
     986973e..6587f91  6587f91ed4d9ef5e974ecf588a7a4b7ba99c6af2 -> main
  To file:/*/$TESTTMP/upstream (glob)
   - [deleted]         gh/test/0/base
   - [deleted]         gh/test/0/head
   - [deleted]         gh/test/0/orig

  $ sl smartlog -T '{node|short} {desc|firstline} {github_pull_request_number}'
  @  6587f91ed4d9 b 1
  │
  ~


Two commit stack that requires pull/rebase since upstream has a newer commit.
  $ touch c
  $ sl commit -Aqm c
  $ touch d
  $ sl commit -Aqm d
  $ sl ghstack submit --short 
  To file:/*/$TESTTMP/upstream (glob)
   * [new branch]      097690c95d5571e8e103a26fc9e3b2487ea9c9f1 -> gh/test/0/head
   * [new branch]      6587f91ed4d9ef5e974ecf588a7a4b7ba99c6af2 -> gh/test/0/base
  To file:/*/$TESTTMP/upstream (glob)
   * [new branch]      6a3fcf68166a0f40b0ac6b59ffa34c159771e85c -> gh/test/1/head
   * [new branch]      097690c95d5571e8e103a26fc9e3b2487ea9c9f1 -> gh/test/1/base
  To file:/*/$TESTTMP/upstream (glob)
   * [new branch]      f4145ca987ad7b3d5c0e733adfebd79602bc423b -> gh/test/0/orig
   * [new branch]      012099ee3b71eedafb0905bb7b9b500b1ea07a99 -> gh/test/1/orig
  https://github.com/facebook/test_github_repo/pull/2
  https://github.com/facebook/test_github_repo/pull/1
  $ sl smartlog -T '{node|short} {desc|firstline} {github_pull_request_number}'
  @  012099ee3b71 d 2
  │
  o  f4145ca987ad c 1
  │
  o  6587f91ed4d9 b 1
  │
  ~

Make a newer commit upstream
  $ cd $TESTTMP/upstream
  $ git checkout -q main
  $ touch z
  $ git add z
  $ git commit -qm z
  $ git checkout -q some-other-branch

Land only the first commit
  $ cd $TESTTMP/client
  $ sl ghstack land https://github.com/facebook/test_github_repo/pull/1
  pulling from file:/*/$TESTTMP/upstream (glob)
  From file:/*/$TESTTMP/upstream (glob)
     6587f91..4fd2a51  4fd2a518c0503ce36515d98ba908c486679a8854 -> remote/main
  To file:/*/$TESTTMP/upstream (glob)
     4fd2a51..a044d43  a044d43b28501875ff456906aee7a53b1e2454f7 -> main
  To file:/*/$TESTTMP/upstream (glob)
   - [deleted]         gh/test/0/base
   - [deleted]         gh/test/0/head
   - [deleted]         gh/test/0/orig
  $ sl smartlog -T '{node|short} {bookmarks} {desc|firstline}'
  o  a044d43b2850  c
  ╷
  ╷ @  012099ee3b71  d
  ╷ │
  ╷ x  f4145ca987ad  c
  ╭─╯
  o  6587f91ed4d9  b
  │
  ~
