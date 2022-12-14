#chg-compatible
#debugruntest-compatible

  $ configure modernclient
  $ setconfig clone.use-rust=true

  $ newrepo
  $ mv .hg .sl

"root" works in a .sl repo.
  $ hg root
  $TESTTMP/repo1

  $ cd ..


  $ mkdir sapling
  $ cd sapling
Init can create a ".sl" repo.
  $ SL_IDENTITY=sl hg init
  $ ls .hg
  $ ls .sl
  00changelog.i
  hgrc.dynamic
  reponame
  requires
  store

  $ cd ..

  $ newremoterepo clone_me_client
  $ setconfig paths.default=test:clone_me
  $ touch foo
  $ hg commit -Aq -m foo
  $ hg push -r . --to master --create -q

Clone can create a ".sl" repo.

  $ cd
  $ sl clone -q test:clone_me cloned
  $ find cloned
  cloned/foo
  $ ls cloned/.hg
  $ ls cloned/.sl
  00changelog.i
  config
  dirstate
  hgrc.dynamic
  reponame
  requires
  store
  treestate
  updateprogress
  wlock.data
  wlock.lock

  $ cd cloned
Status works in ".sl" repo
  $ LOG=configloader::hg=info hg status -A
   INFO configloader::hg: loading config repo_path=$TESTTMP/cloned
  C foo
  $ cd ..

Test repo config loading
  $ mkdir for_testing_dothg_hgrc
  $ cd for_testing_dothg_hgrc
  $ hg init
  $ cat >> .hg/hgrc <<EOF
  > [foo]
  > bar=baz
  > EOF
  $ hg config foo.bar --debug
  $TESTTMP/for_testing_dothg_hgrc/.hg/hgrc:2: baz
  $ mv .hg/hgrc .hg/config
  $ hg config foo.bar --debug
  [1]
  $ cd ..
  $ mkdir for_testing_dotsl_config
  $ cd for_testing_dotsl_config
  $ sl init
  $ cp ../for_testing_dothg_hgrc/.hg/config .sl/config
  $ hg config foo.bar --debug
  $TESTTMP/for_testing_dotsl_config/.sl/config:2: baz
  $ mv .sl/config .sl/hgrc
  $ hg config foo.bar --debug
  [1]

Test we prefer ".sl" over ".hg"
  $ HGIDENTITY=sl newrepo
  $ mkdir .hg
  $ hg root --dotdir
  $TESTTMP/repo2/.sl
