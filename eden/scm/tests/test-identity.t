
#require no-eden


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
  ls: .hg: $ENOENT$
  [1]
  $ ls .sl
  00changelog.i
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
  ls: cloned/.hg: $ENOENT$
  [1]
  $ ls cloned/.sl
  00changelog.i
  config
  dirstate
  namejournal
  namejournal_lock.data
  namejournal_lock.lock
  reponame
  requires
  store
  treestate
  wlock.data
  wlock.lock

  $ cd cloned
Status works in ".sl" repo
  $ LOG=configloader::hg=info hg status -A
   INFO configloader::hg: loading config repo_path=* (glob)
   WARN configloader::hg: repo name: no remotefilelog.reponame
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

Can choose flavor of dot dir using REPO_IDENTITY override:
  $ SL_IDENTITY=sl SL_REPO_IDENTITY=hg hg version -q
  Sapling 4.4.2_dev
  $ SL_IDENTITY=sl SL_REPO_IDENTITY=hg newrepo
  $ ls .hg/requires
  .hg/requires
Works from within a repo of the opposite flavor:
  $ SL_REPO_IDENTITY=sl hg init foo
  $ ls foo/.sl/requires
  foo/.sl/requires


Export/import works:
  $ newrepo
  $ echo "A" | drawdag
  $ HGIDENTITY=sl hg export -r $A | hg import -
  applying patch from stdin
  $ hg show
  commit:      426bada5c675
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A
  description:
  A
  
  
  diff -r 000000000000 -r 426bada5c675 A
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +A
  \ No newline at end of file
