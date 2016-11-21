
  $ . $TESTDIR/require-ext.sh evolve
  $ setupevolve() {
  > cat << EOF >> .hg/hgrc
  > [extensions]
  > evolve=
  > [experimental]
  > evolution=createmarkers
  > evolutioncommands=obsolete
  > EOF
  > }
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon

Setup server
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Backup empty repo
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client
  $ setupevolve
  $ hg debugbackup
  nothing to backup
  $ mkcommit commit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 000000000000
  1 changesets pruned
  $ mkcommit newcommit
  $ hg debugbackup
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     606a357e69ad  newcommit

Re-clone the client
  $ cd ..
  $ rm -rf client
  $ hg clone ssh://user@dummy/repo client -q
  $ cd client

Setup client
  $ setupevolve

Make commit and backup it
  $ mkcommit commit
  $ hg debugbackup
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     7e6a6fd9c7c8  commit
  $ scratchnodes
  606a357e69adb2e36d559ae3237626e82a955c9d 2a40dbb1b839c5720ba2662ca1329373674a6e3a
  7e6a6fd9c7c8c8c307ee14678f03d63af3a7b455 6ff0097c5e81598752887c6990977a3a1f981004
  $ cat .hg/store/infinitepushbackuptip
  0 (no-eol)

Make first commit public (by doing push) and then backup new commit
  $ hg push
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  $ mkcommit newcommit
  $ hg debugbackup
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     94a60f5ad8b2  newcommit
  $ cat .hg/store/infinitepushbackuptip
  1 (no-eol)
Create obsoleted commit
  $ mkcommit obsoletedcommit
  $ hg prune .
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  working directory now at 94a60f5ad8b2
  1 changesets pruned

Make obsoleted commit non-extinct by committing on top of it
  $ hg --hidden up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory parent is obsolete!
  $ mkcommit ontopofobsoleted
  1 new unstable changesets

Backup both of them
  $ hg debugbackup
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 3 commits:
  remote:     94a60f5ad8b2  newcommit
  remote:     361e89f06232  obsoletedcommit
  remote:     d5609f7fa633  ontopofobsoleted
  $ cat .hg/store/infinitepushbackuptip
  3 (no-eol)

Create one more head and run `hg debugbackup`. Make sure that only new head is pushed
  $ hg up 0
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ mkcommit newhead
  created new head
  $ hg debugbackup
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 1 commit:
  remote:     3a30e220fe42  newhead

Create two more heads and backup them
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead1
  created new head
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkcommit newhead2
  created new head
  $ hg backup
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: pushing 2 commits:
  remote:     f79c5017def3  newhead1
  remote:     667453c0787e  newhead2

Backup in background
  $ cat .hg/store/infinitepushbackuptip
  6 (no-eol)
  $ mkcommit newcommit
  $ tip=`hg log -r tip -T '{rev}'`
  $ hg backup --background
  >>> from time import sleep
  >>> for i in range(5):
  ...   sleep(0.1)
  ...   backuptip = int(open('.hg/store/infinitepushbackuptip').read())
  ...   if backuptip == 7:
  ...     break
  $ cat .hg/store/infinitepushbackuptip
  7 (no-eol)
