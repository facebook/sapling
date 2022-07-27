#chg-compatible

test rust clone

  $ configure modern
  $ setconfig clone.use-rust=True
  $ setconfig remotefilelog.reponame=test-repo
  $ setconfig format.use-eager-repo=True
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
  $ hg clone -U -r $D test:e1 $TESTTMP/rev-clone
  fetching lazy changelog
  populating main commit graph
  tip commit: 9bc730a19041f9ec7cb33c626e811aa233efb18c
  fetching selected remote bookmarks

  $ git init -q git-source
  $ hg clone --git "$TESTTMP/git-source" $TESTTMP/git-clone

Test rust clone
  $ hg clone -Uq test:e1 $TESTTMP/rust-clone --config remotenames.selectivepulldefault='master, stable'
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: performing rust clone
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
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: performing rust clone
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
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: performing rust clone
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
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: performing rust clone
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
  abort: .hg directory already exists at clone destination $TESTTMP/failure-clone
  [255]
  $ [ -d $TESTTMP/failure-clone/.hg ]

Test default-destination-dir
  $ hg clone -Uq test:e1 --config clone.default-destination-dir="$TESTTMP/manually-set-dir"
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: performing rust clone
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
   INFO clone_metadata{repo="test-repo-notquite"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo-notquite"}: hgcommands::commands::clone: performing rust clone
  TRACE clone_metadata{repo="test-repo-notquite"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo-notquite"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit

Not an error for bookmarks to not exist
  $ hg clone -Uq test:e1 $TESTTMP/no-bookmarks --config remotenames.selectivepulldefault=banana
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: enter
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: performing rust clone
  TRACE clone_metadata{repo="test-repo"}: hgcommands::commands::clone: fetching lazy commit data and bookmarks
   INFO clone_metadata{repo="test-repo"}: hgcommands::commands::clone: exit
   INFO get_update_target: hgcommands::commands::clone: enter
   INFO get_update_target: hgcommands::commands::clone: return=None
   INFO get_update_target: hgcommands::commands::clone: exit

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
