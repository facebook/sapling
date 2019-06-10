  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution= experimental.bundle2lazylocking=True

# These are necessary to trigger pushkey handlers which may try to take the lock
  $ setconfig devel.legacy.exchange=bookmarks,phases

Create an ondisk bundlestore in .hg/scratchbranches
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ cp $HGRCPATH $TESTTMP/defaulthgrc
  $ setupcommon
  $ enable infinitepush pushrebase
  $ hg init repo
  $ cd repo

Check that we can send a scratch on the server and it does not show there in
the history but is stored on disk
  $ setupserver
  $ cd ..
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ mkcommit initialcommit
  $ hg push -r . --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ mkcommit scratchcommit

  $ rm -rf ../repo/.hg/store/undo*
  $ hg push -r . --to scratch/mybranch --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     20759b6926ce  scratchcommit
# Check if a lock was taken
  $ test -f ../repo/.hg/store/undo
  [1]

  $ hg log -G
  @  changeset:   1:20759b6926ce
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     scratchcommit
  |
  o  changeset:   0:67145f466344
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initialcommit
  
  $ hg log -G -R ../repo
  o  changeset:   0:67145f466344
     tag:         tip
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initialcommit
  
  $ find ../repo/.hg/scratchbranches | sort
  ../repo/.hg/scratchbranches
  ../repo/.hg/scratchbranches/filebundlestore
  ../repo/.hg/scratchbranches/filebundlestore/b9
  ../repo/.hg/scratchbranches/filebundlestore/b9/e1
  ../repo/.hg/scratchbranches/filebundlestore/b9/e1/b9e1ee5f93fb6d7c42496fc176c09839639dd9cc
  ../repo/.hg/scratchbranches/index
  ../repo/.hg/scratchbranches/index/bookmarkmap
  ../repo/.hg/scratchbranches/index/bookmarkmap/scratch
  ../repo/.hg/scratchbranches/index/bookmarkmap/scratch/mybranch
  ../repo/.hg/scratchbranches/index/nodemap
  ../repo/.hg/scratchbranches/index/nodemap/20759b6926ce827d5a8c73eb1fa9726d6f7defb2

From another client we can get the scratchbranch if we ask for it explicitely

  $ cd ..
  $ hg clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ hg pull -B scratch/mybranch --traceback
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 20759b6926ce
  $ hg log -G
  o  changeset:   1:20759b6926ce
  |  bookmark:    scratch/mybranch
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     scratchcommit
  |
  @  changeset:   0:67145f466344
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     initialcommit
  
  $ cd ..

Push to non-scratch bookmark

  $ cd client
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newcommit
  $ hg push -r .
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  @  newcommit public
  |
  | o  scratchcommit draft
  |/
  o  initialcommit public
  

Push to scratch branch
  $ cd ../client2
  $ hg up -q scratch/mybranch
  $ mkcommit 'new scratch commit'
  $ hg push -r . --to scratch/mybranch
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 2 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  @  new scratch commit draft scratch/mybranch
  |
  o  scratchcommit draft
  |
  o  initialcommit public
  
  $ scratchnodes
  1de1d7d92f8965260391d0513fe8a8d5973d3042 bed63daed3beba97fff2e819a148cf415c217a85
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 bed63daed3beba97fff2e819a148cf415c217a85

  $ scratchbookmarks
  scratch/mybranch 1de1d7d92f8965260391d0513fe8a8d5973d3042

Push scratch bookmark with no new revs
  $ hg push -r . --to scratch/anotherbranch --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 2 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  @  new scratch commit draft scratch/mybranch
  |
  o  scratchcommit draft
  |
  o  initialcommit public
  
  $ scratchbookmarks
  scratch/anotherbranch 1de1d7d92f8965260391d0513fe8a8d5973d3042
  scratch/mybranch 1de1d7d92f8965260391d0513fe8a8d5973d3042

Pull scratch and non-scratch bookmark at the same time

  $ hg -R ../repo book newbook
  $ cd ../client
  $ hg pull -B newbook -B scratch/mybranch --traceback
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  adding remote bookmark newbook
  new changesets 1de1d7d92f89
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  o  new scratch commit draft scratch/mybranch
  |
  | @  newcommit public
  | |
  o |  scratchcommit draft
  |/
  o  initialcommit public
  

Push scratch revision without bookmark with --bundle-store

  $ hg up -q tip
  $ mkcommit scratchcommitnobook
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  @  scratchcommitnobook draft
  |
  o  new scratch commit draft scratch/mybranch
  |
  | o  newcommit public
  | |
  o |  scratchcommit draft
  |/
  o  initialcommit public
  
  $ hg push -r . --bundle-store
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 3 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit
  remote:     2b5d271c7e0d  scratchcommitnobook
  $ hg -R ../repo log -G -T '{desc} {phase}'
  o  newcommit public
  |
  o  initialcommit public
  

  $ scratchnodes
  1de1d7d92f8965260391d0513fe8a8d5973d3042 66fa08ff107451320512817bed42b7f467a1bec3
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 66fa08ff107451320512817bed42b7f467a1bec3
  2b5d271c7e0d25d811359a314d413ebcc75c9524 66fa08ff107451320512817bed42b7f467a1bec3

Test with pushrebase
  $ cp $TESTTMP/defaulthgrc $HGRCPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > pushrebase=
  > infinitepush=
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF
  $ mkcommit scratchcommitwithpushrebase
  $ hg push -r . --to scratch/mybranch
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 4 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit
  remote:     2b5d271c7e0d  scratchcommitnobook
  remote:     d8c4f54ab678  scratchcommitwithpushrebase
  $ hg -R ../repo log -G -T '{desc} {phase}'
  o  newcommit public
  |
  o  initialcommit public
  
  $ scratchnodes
  1de1d7d92f8965260391d0513fe8a8d5973d3042 e3cb2ac50f9e1e6a5ead3217fc21236c84af4397
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 e3cb2ac50f9e1e6a5ead3217fc21236c84af4397
  2b5d271c7e0d25d811359a314d413ebcc75c9524 e3cb2ac50f9e1e6a5ead3217fc21236c84af4397
  d8c4f54ab678fd67cb90bb3f272a2dc6513a59a7 e3cb2ac50f9e1e6a5ead3217fc21236c84af4397

Change the order of pushrebase and infinitepush
  $ cp $TESTTMP/defaulthgrc $HGRCPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > infinitepush=
  > pushrebase=
  > [infinitepush]
  > branchpattern=re:scratch/.+
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > EOF
  $ mkcommit scratchcommitwithpushrebase2
  $ hg push -r . --to scratch/mybranch
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 5 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit
  remote:     2b5d271c7e0d  scratchcommitnobook
  remote:     d8c4f54ab678  scratchcommitwithpushrebase
  remote:     6c10d49fe927  scratchcommitwithpushrebase2
  $ hg -R ../repo log -G -T '{desc} {phase}'
  o  newcommit public
  |
  o  initialcommit public
  
  $ scratchnodes
  1de1d7d92f8965260391d0513fe8a8d5973d3042 cd0586065eaf8b483698518f5fc32531e36fd8e0
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 cd0586065eaf8b483698518f5fc32531e36fd8e0
  2b5d271c7e0d25d811359a314d413ebcc75c9524 cd0586065eaf8b483698518f5fc32531e36fd8e0
  6c10d49fe92751666c40263f96721b918170d3da cd0586065eaf8b483698518f5fc32531e36fd8e0
  d8c4f54ab678fd67cb90bb3f272a2dc6513a59a7 cd0586065eaf8b483698518f5fc32531e36fd8e0

Non-fastforward scratch bookmark push
  $ hg up 6c10d49fe927
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 > amend
  $ hg add amend
  $ hg ci --amend -m 'scratch amended commit'
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/6c10d49fe927-c99ffec5-amend.hg (glob)
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  @  scratch amended commit draft
  |
  o  scratchcommitwithpushrebase draft
  |
  o  scratchcommitnobook draft
  |
  o  new scratch commit draft scratch/mybranch
  |
  | o  newcommit public
  | |
  o |  scratchcommit draft
  |/
  o  initialcommit public
  

  $ scratchbookmarks
  scratch/anotherbranch 1de1d7d92f8965260391d0513fe8a8d5973d3042
  scratch/mybranch 6c10d49fe92751666c40263f96721b918170d3da
  $ hg push -r . --to scratch/mybranch
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: non-forward push
  remote: (use --non-forward-move to override)
  abort: push failed on remote
  [255]

  $ hg push -r . --to scratch/mybranch --non-forward-move
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 5 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit
  remote:     2b5d271c7e0d  scratchcommitnobook
  remote:     d8c4f54ab678  scratchcommitwithpushrebase
  remote:     8872775dd97a  scratch amended commit
  $ scratchbookmarks
  scratch/anotherbranch 1de1d7d92f8965260391d0513fe8a8d5973d3042
  scratch/mybranch 8872775dd97a750e1533dc1fbbca665644b32547
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  @  scratch amended commit draft
  |
  o  scratchcommitwithpushrebase draft
  |
  o  scratchcommitnobook draft
  |
  o  new scratch commit draft scratch/mybranch
  |
  | o  newcommit public
  | |
  o |  scratchcommit draft
  |/
  o  initialcommit public
  
Check that push path is not ignored. Add new path to the hgrc
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > peer=ssh://user@dummy/client2
  > EOF

Checkout last non-scrath commit
  $ hg up 91894e11e8255
  1 files updated, 0 files merged, 6 files removed, 0 files unresolved
  $ mkcommit peercommit
Use --force because this push creates new head
  $ hg push peer -r . -f
  pushing to ssh://user@dummy/client2
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 2 files (+1 heads)
  $ hg -R ../repo log -G -T '{desc} {phase} {bookmarks}'
  o  newcommit public
  |
  o  initialcommit public
  
  $ hg -R ../client2 log -G -T '{desc} {phase} {bookmarks}'
  o  peercommit public
  |
  o  newcommit public
  |
  | @  new scratch commit draft scratch/mybranch
  | |
  | o  scratchcommit draft
  |/
  o  initialcommit public
  
  $ hg book --list-remote scratch/*
     scratch/anotherbranch     1de1d7d92f8965260391d0513fe8a8d5973d3042
     scratch/mybranch          8872775dd97a750e1533dc1fbbca665644b32547
  $ hg book --list-remote
  abort: --list-remote requires a bookmark pattern
  (use "hg book" to get a list of your local bookmarks)
  [255]
  $ hg book --config infinitepush.defaultremotepatterns=scratch/another* --list-remote
  abort: --list-remote requires a bookmark pattern
  (use "hg book" to get a list of your local bookmarks)
  [255]
  $ hg book --list-remote scratch/my
  $ hg book --list-remote scratch/my*
     scratch/mybranch          8872775dd97a750e1533dc1fbbca665644b32547
  $ hg book --list-remote scratch/my* -T json
  [
   {
    "bookmark": "scratch/mybranch",
    "node": "8872775dd97a750e1533dc1fbbca665644b32547"
   }
  ]
  $ cd ../repo
  $ hg book scratch/serversidebook
  $ hg book serversidebook
  $ cd ../client
  $ hg book --list-remote scratch/* -T json
  [
   {
    "bookmark": "scratch/anotherbranch",
    "node": "1de1d7d92f8965260391d0513fe8a8d5973d3042"
   },
   {
    "bookmark": "scratch/mybranch",
    "node": "8872775dd97a750e1533dc1fbbca665644b32547"
   },
   {
    "bookmark": "scratch/serversidebook",
    "node": "0000000000000000000000000000000000000000"
   }
  ]

Push to svn server should fail
  $ hg push svn+ssh://svn.vip.facebook.com/repo -r . --to scratch/serversidebook
  abort: infinite push does not work with svn repo
  (did you forget to `hg push default`?)
  [255]

Scratch pull of pruned commits
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > amend=
  > [experimental]
  > evolution=createmarkers
  > EOF
  $ hg book -d scratch/mybranch
  $ hg hide 8872775dd97a
  hiding commit 8872775dd97a "scratch amended commit"
  1 changeset hidden
  $ hg pull -B scratch/mybranch
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 6 files
  adding remote bookmark scratch/serversidebook
  adding remote bookmark serversidebook
  $ hg log -r 'reverse(::scratch/mybranch)' -T '{desc}\n'
  scratch amended commit
  scratchcommitwithpushrebase
  scratchcommitnobook
  new scratch commit
  scratchcommit
  initialcommit

Prune it again and pull it via commit hash
  $ hg log -r scratch/mybranch -T '{node}\n'
  8872775dd97a750e1533dc1fbbca665644b32547
  $ hg prune -r scratch/mybranch
  1 changesets pruned
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg log -G -T '{node|short} {desc} {bookmarks}'
  @  fe8283fe1190 peercommit
  |
  | o  d8c4f54ab678 scratchcommitwithpushrebase scratch/mybranch
  | |
  | o  2b5d271c7e0d scratchcommitnobook
  | |
  | o  1de1d7d92f89 new scratch commit
  | |
  o |  91894e11e825 newcommit
  | |
  | o  20759b6926ce scratchcommit
  |/
  o  67145f466344 initialcommit
  
Have to use full hash because short hashes are not supported yet
  $ hg pull -r 8872775dd97a750e1533dc1fbbca665644b32547
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 6 files
  $ hg log -G -T '{node|short} {desc} {bookmarks}'
  @  fe8283fe1190 peercommit
  |
  | o  8872775dd97a scratch amended commit
  | |
  | o  d8c4f54ab678 scratchcommitwithpushrebase scratch/mybranch
  | |
  | o  2b5d271c7e0d scratchcommitnobook
  | |
  | o  1de1d7d92f89 new scratch commit
  | |
  o |  91894e11e825 newcommit
  | |
  | o  20759b6926ce scratchcommit
  |/
  o  67145f466344 initialcommit
  
Push new scratch head. Make sure that new bundle is created but 8872775dd97a
still in the old bundle
  $ hg up scratch/mybranch
  4 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (activating bookmark scratch/mybranch)
  $ mkcommit newscratchhead
  $ hg push -r . --to scratch/newscratchhead --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 5 commits:
  remote:     20759b6926ce  scratchcommit
  remote:     1de1d7d92f89  new scratch commit
  remote:     2b5d271c7e0d  scratchcommitnobook
  remote:     d8c4f54ab678  scratchcommitwithpushrebase
  remote:     8611afacb870  newscratchhead
  $ scratchnodes
  1de1d7d92f8965260391d0513fe8a8d5973d3042 d1b4f12087a79b2b1d342e222686a829d09d399b
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 d1b4f12087a79b2b1d342e222686a829d09d399b
  2b5d271c7e0d25d811359a314d413ebcc75c9524 d1b4f12087a79b2b1d342e222686a829d09d399b
  6c10d49fe92751666c40263f96721b918170d3da cd0586065eaf8b483698518f5fc32531e36fd8e0
  8611afacb87078300a6d6b2f0c4b49fa506a8db9 d1b4f12087a79b2b1d342e222686a829d09d399b
  8872775dd97a750e1533dc1fbbca665644b32547 ac7f12436d58e685616ffc1f619bcecce8829e25
  d8c4f54ab678fd67cb90bb3f272a2dc6513a59a7 d1b4f12087a79b2b1d342e222686a829d09d399b

Recreate the repo
  $ cd ..
  $ rm -rf repo
  $ hg init repo
  $ cd repo
  $ setupserver
  $ mkcommit initialcommit
  $ hg phase --public .

Recreate the clients
  $ cd ..
  $ rm -rf client
  $ rm -rf client2
  $ hg clone ssh://user@dummy/repo client -q

Create two heads. Push first head alone, then two heads together. Make sure that
multihead push works.
  $ cd client
  $ mkcommit multihead1
  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ mkcommit multihead2
  $ hg push -r . --bundle-store
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     ee4802bf6864  multihead2
  $ hg push -r '1:2' --bundle-store
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 2 commits:
  remote:     bc22f9a30a82  multihead1
  remote:     ee4802bf6864  multihead2
  $ scratchnodes
  bc22f9a30a821118244deacbd732e394ed0b686c ab1bc557aa090a9e4145512c734b6e8a828393a5
  ee4802bf6864326a6b3dcfff5a03abc2a0a69b8f ab1bc557aa090a9e4145512c734b6e8a828393a5

Create two new scratch bookmarks
  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit scratchfirstpart
  $ hg push -r . --to scratch/firstpart --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     176993b87e39  scratchfirstpart
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit scratchsecondpart
  $ hg push -r . --to scratch/secondpart --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     8db3891c220e  scratchsecondpart

Pull two bookmarks from the second client
  $ cd ..
  $ hg clone ssh://user@dummy/repo client2 -q
  $ cd client2
  $ hg pull -B scratch/firstpart -B scratch/secondpart
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets * (glob)
  $ hg log -r scratch/secondpart -T '{node}'
  8db3891c220e216f6da214e8254bd4371f55efca (no-eol)
  $ hg log -r scratch/firstpart -T '{node}'
  176993b87e39bd88d66a2cccadabe33f0b346339 (no-eol)
Make two commits to the scratch branch
  $ mkcommit testpullbycommithash1
  $ hg log -r '.' -T '{node}\n' > ../testpullbycommithash1
  $ mkcommit testpullbycommithash2
  $ hg push -r . --to scratch/mybranch --create -q

Create third client and pull by commit hash.
Make sure testpullbycommithash2 has not fetched
  $ cd ..
  $ hg clone ssh://user@dummy/repo client3 -q
  $ cd client3
  $ hg pull -r `cat ../testpullbycommithash1`
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 33910bfe6ffe
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  o  testpullbycommithash1 draft
  |
  @  initialcommit public
  
Make public commit in the repo and pull it.
Make sure phase on the client is public.
  $ cd ../repo
  $ mkcommit publiccommit
  $ hg phase --public .
  $ cd ../client3
  $ hg pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets a79b6597f322
  $ hg log -G -T '{desc} {phase} {bookmarks} {node|short}'
  o  publiccommit public  a79b6597f322
  |
  | o  testpullbycommithash1 draft  33910bfe6ffe
  |/
  @  initialcommit public  67145f466344
  
  $ hg up a79b6597f322
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkcommit scratchontopofpublic
  $ hg push -r . --to scratch/scratchontopofpublic --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     c70aee6da07d  scratchontopofpublic
  $ cd ../client2
  $ hg pull -B scratch/scratchontopofpublic
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets a79b6597f322:c70aee6da07d
  $ hg log -r scratch/scratchontopofpublic -T '{phase}'
  draft (no-eol)
Strip scratchontopofpublic commit and do hg update
  $ hg log -r tip -T '{node}\n'
  c70aee6da07d7cdb9897375473690df3a8563339
  $ hg debugstrip -q tip
  $ hg up c70aee6da07d7cdb9897375473690df3a8563339
  'c70aee6da07d7cdb9897375473690df3a8563339' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets c70aee6da07d
  'c70aee6da07d7cdb9897375473690df3a8563339' found remotely
  pull finished in * sec (glob)
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved

Trying to pull from bad path
  $ hg debugstrip -q tip
  $ hg --config paths.default=badpath up c70aee6da07d7cdb9897375473690df3a8563339
  'c70aee6da07d7cdb9897375473690df3a8563339' does not exist locally - looking for it remotely...
  pulling from $TESTTMP/client2/badpath (glob)
  pull failed: repository $TESTTMP/client2/badpath not found
  abort: unknown revision 'c70aee6da07d7cdb9897375473690df3a8563339'!
  (if c70aee6da07d7cdb9897375473690df3a8563339 is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

Strip commit and pull it using hg update with bookmark name
  $ hg debugstrip -q d8fde0ddfc96
  $ hg up scratch/mybranch
  'scratch/mybranch' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 2 files
  new changesets d8fde0ddfc96
  'scratch/mybranch' found remotely
  pull finished in * sec (glob)
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark scratch/mybranch)
  $ hg log -r scratch/mybranch -T '{node}'
  d8fde0ddfc962183977f92d2bc52d303b8840f9d (no-eol)

Test debugfillinfinitepushmetadata
  $ cd ../repo
  $ hg debugfillinfinitepushmetadata
  abort: nodes are not specified
  [255]
  $ hg debugfillinfinitepushmetadata --node randomnode
  abort: node randomnode is not found
  [255]
  $ hg debugfillinfinitepushmetadata --node d8fde0ddfc962183977f92d2bc52d303b8840f9d
  $ cat .hg/scratchbranches/index/nodemetadatamap/d8fde0ddfc962183977f92d2bc52d303b8840f9d
  {"changed_files": {"testpullbycommithash2": {"adds": 1, "isbinary": false, "removes": 0, "status": "added"}}} (no-eol)

  $ cd ../client
  $ hg up d8fde0ddfc962183977f92d2bc52d303b8840f9d
  'd8fde0ddfc962183977f92d2bc52d303b8840f9d' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  new changesets 33910bfe6ffe:d8fde0ddfc96
  'd8fde0ddfc962183977f92d2bc52d303b8840f9d' found remotely
  pull finished in * sec (glob)
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo file > file
  $ hg add file
  $ hg rm testpullbycommithash2
  $ hg ci -m 'add and rm files'
  $ hg log -r . -T '{node}\n'
  3edfe7e9089ab9f728eb8e0d0c62a5d18cf19239
  $ hg cp file cpfile
  $ hg mv file mvfile
  $ hg ci -m 'cpfile and mvfile'
  $ hg log -r . -T '{node}\n'
  c7ac39f638c6b39bcdacf868fa21b6195670f8ae
  $ hg push -r . --bundle-store
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 4 commits:
  remote:     33910bfe6ffe  testpullbycommithash1
  remote:     d8fde0ddfc96  testpullbycommithash2
  remote:     3edfe7e9089a  add and rm files
  remote:     c7ac39f638c6  cpfile and mvfile
  $ cd ../repo
  $ hg debugfillinfinitepushmetadata --node 3edfe7e9089ab9f728eb8e0d0c62a5d18cf19239 --node c7ac39f638c6b39bcdacf868fa21b6195670f8ae
  $ cat .hg/scratchbranches/index/nodemetadatamap/3edfe7e9089ab9f728eb8e0d0c62a5d18cf19239
  {"changed_files": {"file": {"adds": 1, "isbinary": false, "removes": 0, "status": "added"}, "testpullbycommithash2": {"adds": 0, "isbinary": false, "removes": 1, "status": "removed"}}} (no-eol)
  $ cat .hg/scratchbranches/index/nodemetadatamap/c7ac39f638c6b39bcdacf868fa21b6195670f8ae
  {"changed_files": {"cpfile": {"adds": 1, "copies": "file", "isbinary": false, "removes": 0, "status": "added"}, "file": {"adds": 0, "isbinary": false, "removes": 1, "status": "removed"}, "mvfile": {"adds": 1, "copies": "file", "isbinary": false, "removes": 0, "status": "added"}}} (no-eol)

Test infinitepush.metadatafilelimit number
  $ cd ../client
  $ echo file > file
  $ hg add file
  $ echo file1 > file1
  $ hg add file1
  $ echo file2 > file2
  $ hg add file2
  $ hg ci -m 'add many files'
  $ hg log -r . -T '{node}'
  09904fb20c53ff351bd3b1d47681f569a4dab7e5 (no-eol)
  $ hg push -r . --bundle-store
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 5 commits:
  remote:     33910bfe6ffe  testpullbycommithash1
  remote:     d8fde0ddfc96  testpullbycommithash2
  remote:     3edfe7e9089a  add and rm files
  remote:     c7ac39f638c6  cpfile and mvfile
  remote:     09904fb20c53  add many files

  $ cd ../repo
  $ hg debugfillinfinitepushmetadata --node 09904fb20c53ff351bd3b1d47681f569a4dab7e5 --config infinitepush.metadatafilelimit=2
  $ cat .hg/scratchbranches/index/nodemetadatamap/09904fb20c53ff351bd3b1d47681f569a4dab7e5
  {"changed_files": {"file": {"adds": 1, "isbinary": false, "removes": 0, "status": "added"}, "file1": {"adds": 1, "isbinary": false, "removes": 0, "status": "added"}}, "changed_files_truncated": true} (no-eol)

Test infinitepush.fillmetadatabranchpattern
  $ cd ../repo
  $ cat >> .hg/hgrc << EOF
  > [infinitepush]
  > fillmetadatabranchpattern=re:scratch/fillmetadata/.*
  > EOF
  $ cd ../client
  $ mkcommit tofillmetadata
  $ hg log -r . -T '{node}\n'
  d2b0410d4da084bc534b1d90df0de9eb21583496
  $ hg push -r . --to scratch/fillmetadata/fill --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 6 commits:
  remote:     33910bfe6ffe  testpullbycommithash1
  remote:     d8fde0ddfc96  testpullbycommithash2
  remote:     3edfe7e9089a  add and rm files
  remote:     c7ac39f638c6  cpfile and mvfile
  remote:     09904fb20c53  add many files
  remote:     d2b0410d4da0  tofillmetadata

Make sure background process finished
  $ sleep 3
  $ cd ../repo
  $ cat .hg/scratchbranches/index/nodemetadatamap/d2b0410d4da084bc534b1d90df0de9eb21583496
  {"changed_files": {"tofillmetadata": {"adds": 1, "isbinary": false, "removes": 0, "status": "added"}}} (no-eol)
