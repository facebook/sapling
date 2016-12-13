
Create an ondisk bundlestore in .hg/scratchbranches
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/pushrebase.py $TESTTMP # use $TESTTMP substitution in message
  $ cp $HGRCPATH $TESTTMP/defaulthgrc
  $ setupcommon
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
  $ hg push -r . --to scratch/mybranch --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     20759b6926ce  scratchcommit
  $ hg log -G
  @  changeset:   1:20759b6926ce
  |  bookmark:    scratch/mybranch
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
  ../repo/.hg/scratchbranches/filebundlestore/67
  ../repo/.hg/scratchbranches/filebundlestore/67/c0
  ../repo/.hg/scratchbranches/filebundlestore/67/c0/67c002fef9703044964834c2bb577a49c5a0b5ec
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
  (run 'hg update' to get a working copy)
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
  created new head
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
  | o  scratchcommit draft scratch/mybranch
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
  1de1d7d92f8965260391d0513fe8a8d5973d3042 19412dd566222618cd030e18005c80c356c9f843
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 19412dd566222618cd030e18005c80c356c9f843

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
  @  new scratch commit draft scratch/anotherbranch scratch/mybranch
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
  (run 'hg update' to get a working copy)
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
  1de1d7d92f8965260391d0513fe8a8d5973d3042 4e165e8b1f81dfbb4d26297dc66988940756fcb0
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 4e165e8b1f81dfbb4d26297dc66988940756fcb0
  2b5d271c7e0d25d811359a314d413ebcc75c9524 4e165e8b1f81dfbb4d26297dc66988940756fcb0

Test with pushrebase
  $ cp $TESTTMP/defaulthgrc $HGRCPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > pushrebase=$TESTTMP/pushrebase.py
  > infinitepush=$TESTTMP/infinitepush
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
  1de1d7d92f8965260391d0513fe8a8d5973d3042 2d0fd81e57a6018f58b63985243ebe00a34de4cb
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 2d0fd81e57a6018f58b63985243ebe00a34de4cb
  2b5d271c7e0d25d811359a314d413ebcc75c9524 2d0fd81e57a6018f58b63985243ebe00a34de4cb
  d8c4f54ab678fd67cb90bb3f272a2dc6513a59a7 2d0fd81e57a6018f58b63985243ebe00a34de4cb

Change the order of pushrebase and infinitepush
  $ cp $TESTTMP/defaulthgrc $HGRCPATH
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > infinitepush=$TESTTMP/infinitepush
  > pushrebase=$TESTTMP/pushrebase.py
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
  1de1d7d92f8965260391d0513fe8a8d5973d3042 eb133b9754ed60fcdf98bd4ddae7e80b6b70fe9e
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 eb133b9754ed60fcdf98bd4ddae7e80b6b70fe9e
  2b5d271c7e0d25d811359a314d413ebcc75c9524 eb133b9754ed60fcdf98bd4ddae7e80b6b70fe9e
  6c10d49fe92751666c40263f96721b918170d3da eb133b9754ed60fcdf98bd4ddae7e80b6b70fe9e
  d8c4f54ab678fd67cb90bb3f272a2dc6513a59a7 eb133b9754ed60fcdf98bd4ddae7e80b6b70fe9e

Non-fastforward scratch bookmark push
  $ hg up 6c10d49fe927
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 > amend
  $ hg add amend
  $ hg ci --amend -m 'scratch amended commit'
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/6c10d49fe927-a7adb791-amend-backup.hg (glob)
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  @  scratch amended commit draft scratch/mybranch
  |
  o  scratchcommitwithpushrebase draft
  |
  o  scratchcommitnobook draft
  |
  o  new scratch commit draft
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
  abort: push failed on remote
  (use --force to override)
  [255]

  $ hg push -r . --to scratch/mybranch --force
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
  @  scratch amended commit draft scratch/mybranch
  |
  o  scratchcommitwithpushrebase draft
  |
  o  scratchcommitnobook draft
  |
  o  new scratch commit draft
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
  | @  new scratch commit draft scratch/anotherbranch scratch/mybranch
  | |
  | o  scratchcommit draft
  |/
  o  initialcommit public
  
  $ hg book --list-remote scratch/*
     scratch/anotherbranch     1de1d7d92f8965260391d0513fe8a8d5973d3042
     scratch/mybranch          8872775dd97a750e1533dc1fbbca665644b32547
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
  $ hg push svn+ssh://svn.vip.facebook.com/svnroot/tfb/trunk/www -r . --to scratch/serversidebook
  abort: infinite push does not work with svn repo
  (did you forget to `hg push default`?)
  [255]

Scratch pull of pruned commits
  $ . $TESTDIR/require-ext.sh inhibit directaccess evolve
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > directaccess=
  > evolve=
  > inhibit=
  > [experimental]
  > evolution=createmarkers
  > evolutioncommands=obsolete
  > EOF
  $ hg prune -r scratch/mybranch
  1 changesets pruned
  $ hg log -r 'reverse(::scratch/mybranch)' -T '{desc}\n'
  scratchcommitwithpushrebase
  scratchcommitnobook
  new scratch commit
  scratchcommit
  initialcommit
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
  created new head
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
  1de1d7d92f8965260391d0513fe8a8d5973d3042 193a989b8ab8773d1cca5a6d1fdcbf5aa9e61b62
  20759b6926ce827d5a8c73eb1fa9726d6f7defb2 193a989b8ab8773d1cca5a6d1fdcbf5aa9e61b62
  2b5d271c7e0d25d811359a314d413ebcc75c9524 193a989b8ab8773d1cca5a6d1fdcbf5aa9e61b62
  6c10d49fe92751666c40263f96721b918170d3da eb133b9754ed60fcdf98bd4ddae7e80b6b70fe9e
  8611afacb87078300a6d6b2f0c4b49fa506a8db9 193a989b8ab8773d1cca5a6d1fdcbf5aa9e61b62
  8872775dd97a750e1533dc1fbbca665644b32547 2e17e57fa00167232df26b8e1752f30d00f052c6
  d8c4f54ab678fd67cb90bb3f272a2dc6513a59a7 193a989b8ab8773d1cca5a6d1fdcbf5aa9e61b62

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
  created new head
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
  bc22f9a30a821118244deacbd732e394ed0b686c 4fb870642394276983606d0b419b87547d121e0e
  ee4802bf6864326a6b3dcfff5a03abc2a0a69b8f 4fb870642394276983606d0b419b87547d121e0e

Create two new scratch bookmarks
  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit scratchfirstpart
  created new head
  $ hg push -r . --to scratch/firstpart --create
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     176993b87e39  scratchfirstpart
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit scratchsecondpart
  created new head
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
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg log -G -T '{desc} {phase} {bookmarks}'
  o  scratchsecondpart draft scratch/secondpart
  |
  | o  scratchfirstpart draft scratch/firstpart
  |/
  @  initialcommit public
  
Make two commits to the scratch branch
  $ mkcommit testpullbycommithash1
  created new head
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
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
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
  (run 'hg heads' to see heads, 'hg merge' to merge)
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
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg log -G -T '{desc} {phase} {bookmarks} {node|short}'
  o  scratchontopofpublic draft scratch/scratchontopofpublic c70aee6da07d
  |
  o  publiccommit public  a79b6597f322
  |
  | @  testpullbycommithash2 draft scratch/mybranch d8fde0ddfc96
  | |
  | o  testpullbycommithash1 draft  33910bfe6ffe
  |/
  | o  scratchsecondpart draft scratch/secondpart 8db3891c220e
  |/
  | o  scratchfirstpart draft scratch/firstpart 176993b87e39
  |/
  o  initialcommit public  67145f466344
  
