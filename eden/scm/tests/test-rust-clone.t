#chg-compatible

test rust clone

  $ eagerepo
  $ setconfig clone.use-rust=True
  $ setconfig remotefilelog.reponame=test-repo
  $ export LOG=hgcommands::commands::clone


 Prepare Source:

  $ newrepo e1
  $ drawdag << 'EOS'
  > E  # bookmark master = E
  > |
  > D
  > |
  > C  # bookmark stable = C
  > |
  > B
  > |
  > A
  > EOS

Test that nonsupported options fallback to python:

  $ cd $TESTTMP
  $ hg clone -U -r $D ~/e1 $TESTTMP/rev-clone
  fetching lazy changelog
  populating main commit graph
  tip commit: 9bc730a19041f9ec7cb33c626e811aa233efb18c
  fetching selected remote bookmarks

  $ git init -q git-source
  $ hg clone --git "$TESTTMP/git-source" $TESTTMP/git-clone

Test rust clone
  $ hg clone -Uq test:e1 $TESTTMP/rust-clone --config remotenames.selectivepulldefault='master, stable'
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit
  $ cd $TESTTMP/rust-clone

Check metalog is written and keys are tracked correctly
  $ hg dbsh -c 'ui.write(str(ml.get("remotenames")))'
  b'9bc730a19041f9ec7cb33c626e811aa233efb18c bookmarks remote/master\n26805aba1e600a82e93661149f2313866a221a7b bookmarks remote/stable\n' (no-eol)

Check configuration
  $ hg paths
  default = test:e1
  $ hg config remotefilelog.reponame
  test-repo

Check commits
  $ hg log -r tip -T "{desc}\n"
  E
  $ hg log -T "{desc}\n"
  E
  D
  C
  B
  A

Check basic operations
  $ hg up master
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo newfile > newfile
  $ hg commit -Aqm 'new commit'

Test cloning with default destination
  $ cd $TESTTMP
  $ hg clone -Uq test:e1
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit
  $ cd test-repo
  $ hg log -r tip -T "{desc}\n"
  E

Test cloning failures

  $ cd $TESTTMP
  $ FAILPOINTS=run::clone=return hg clone -Uq test:e1 $TESTTMP/failure-clone
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
  ERROR clone_metadata{repo="test-repo"}: hgcommands::commands::clone: error=Injected clone failure
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
  abort: Injected clone failure
  [255]
  $ [ -d $TESTTMP/failure-clone ]
  [1]

Check that preexisting directory is not removed in failure case
  $ mkdir failure-clone
  $ FAILPOINTS=run::clone=return hg clone -Uq test:e1 $TESTTMP/failure-clone
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
  ERROR clone_metadata{repo="test-repo"}: hgcommands::commands::clone: error=Injected clone failure
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
  abort: Injected clone failure
  [255]
  $ [ -d $TESTTMP/failure-clone ]
  $ [ -d $TESTTMP/failure-clone/.hg ]
  [1]

Check that prexisting repo is not modified
  $ mkdir $TESTTMP/failure-clone/.hg
  $ hg clone -Uq test:e1 $TESTTMP/failure-clone
  TRACE hgcommands::commands::clone: performing rust clone
  abort: .hg directory already exists at clone destination $TESTTMP/failure-clone
  [255]
  $ [ -d $TESTTMP/failure-clone/.hg ]

Test default-destination-dir
  $ hg clone -Uq test:e1 --config clone.default-destination-dir="$TESTTMP/manually-set-dir"
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit
  $ ls $TESTTMP | grep manually-set-dir
  manually-set-dir

Test that we get an error when not specifying a destination directory and running in plain mode
  $ HGPLAIN=1 hg clone -Uq test:e1
  abort: DEST must be specified because HGPLAIN is enabled
  [255]
  $ HGPLAINEXCEPT=default_clone_dir hg clone -Uq test:e1 --config remotefilelog.reponame=test-repo-notquite
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo-notquite"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo-notquite"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo-notquite"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit

Not an error for bookmarks to not exist
  $ hg clone -Uq test:e1 $TESTTMP/no-bookmarks --config remotenames.selectivepulldefault=banana
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit
remotenames.selectivepulldefault gets persisted
  $ hg -R $TESTTMP/no-bookmarks config remotenames.selectivepulldefault
  banana

Can specify selectivepull branch via URL fragment:
  $ hg clone -Uq test:e1#banana $TESTTMP/fragment
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit
  $ hg -R $TESTTMP/fragment config remotenames.selectivepulldefault
  banana
  $ hg -R $TESTTMP/fragment paths
  default = test:e1

Test various --eden errors:
  $ hg clone -Uq test:e1 --eden-backing-repo /foo/bar
  abort: --eden-backing-repo requires --eden
  [255]
  $ hg clone -q test:e1 --eden --enable-profile foo
  abort: --enable-profile is not compatible with --eden
  [255]
  $ hg clone -q test:e1 -u foo --eden
  abort: some specified options are not compatible with --eden
  [255]
  $ hg clone -Uq test:e1 --eden
  abort: --noupdate is not compatible with --eden
  [255]
  $ hg clone -q test:e1 --eden --config clone.use-rust=0
  abort: --eden requires --config clone.use-rust=True
  [255]

Don't delete repo on error if --debug:
  $ FAILPOINTS=run::clone=return hg clone -Uq test:e1 $TESTTMP/debug-failure --debug &>/dev/null
  [255]
  $ ls $TESTTMP/debug-failure

Can clone eagerepo without scheme:
  $ cd
  $ hg clone --shallow ./e1 no_scheme
  Cloning test-repo into $TESTTMP/no_scheme
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=Some((HgId("9bc730a19041f9ec7cb33c626e811aa233efb18c"), "master"))
   INFO get_update_target: hgcommands::commands::clone: exit
  Checking out 'master'
  5 files updated
  $ grep remote no_scheme/.hg/requires
  remotefilelog
Make sure we wrote out the absolute path.
  $ hg -R no_scheme config paths.default
  $TESTTMP/e1

Can clone non-shallow:
  $ hg clone ./e1 non_shallow
  Cloning test-repo into $TESTTMP/non_shallow
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=Some((HgId("9bc730a19041f9ec7cb33c626e811aa233efb18c"), "master"))
   INFO get_update_target: hgcommands::commands::clone: exit
  Checking out 'master'
  5 files updated
  $ grep eager non_shallow/.hg/store/requires
  eagerepo

Can pick bookmark or commit using -u:
  $ hg clone -u $D test:e1 d_clone --config experimental.rust-clone-updaterev=true
  Cloning test-repo into $TESTTMP/d_clone
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=Some((HgId("f585351a92f85104bff7c284233c338b10eb1df7"), "f585351a92f85104bff7c284233c338b10eb1df7"))
   INFO get_update_target: hgcommands::commands::clone: exit
  Checking out 'f585351a92f85104bff7c284233c338b10eb1df7'
  4 files updated
  $ hg whereami -R d_clone
  f585351a92f85104bff7c284233c338b10eb1df7

  $ hg clone -u stable test:e1 stable_clone --config remotenames.selectivepulldefault='master, stable' --config experimental.rust-clone-updaterev=true
  Cloning test-repo into $TESTTMP/stable_clone
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=Some((HgId("26805aba1e600a82e93661149f2313866a221a7b"), "stable"))
   INFO get_update_target: hgcommands::commands::clone: exit
  Checking out 'stable'
  3 files updated
  $ hg whereami -R stable_clone
  26805aba1e600a82e93661149f2313866a221a7b


Default to "tip" if selectivepulldefault not available.
  $ hg clone --no-shallow ./e1 no_bookmark --config remotenames.selectivepulldefault=banana
  Cloning test-repo into $TESTTMP/no_bookmark
  TRACE hgcommands::commands::clone: performing rust clone
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
  Server has no 'banana' bookmark - trying tip.
   INFO get_update_target: hgcommands::commands::clone: return=Some((HgId("9bc730a19041f9ec7cb33c626e811aa233efb18c"), "tip"))
   INFO get_update_target: hgcommands::commands::clone: exit
  Checking out 'tip'
  5 files updated

Don't perform any queries for null commit id.
  $ LOG= hg clone -Uq ./e1 no_workingcopy
  $ cd no_workingcopy
  $ LOG=trace hg status -m 2>trace
  $ grep 0000000000000000000000000000000000000000 trace
  [1]
