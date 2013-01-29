Test changesets filtering during exchanges (some tests are still in
test-obsolete.t)

  $ cat > obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH

Push does not corrupt remote
----------------------------

Create a DAG where a changeset reuses a revision from a file first used in an
extinct changeset.

  $ hg init local
  $ cd local
  $ echo 'base' > base
  $ hg commit -Am base
  adding base
  $ echo 'A' > A
  $ hg commit -Am A
  adding A
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg revert -ar 1
  adding A
  $ hg commit -Am "A'"
  created new head
  $ hg log -G --template='{desc} {node}'
  @  A' f89bcc95eba5174b1ccc3e33a82e84c96e8338ee
  |
  | o  A 9d73aac1b2ed7d53835eaeec212ed41ea47da53a
  |/
  o  base d20a80d4def38df63a4b330b7fb688f3d4cae1e3
  
  $ hg debugobsolete 9d73aac1b2ed7d53835eaeec212ed41ea47da53a f89bcc95eba5174b1ccc3e33a82e84c96e8338ee

Push it. The bundle should not refer to the extinct changeset.

  $ hg init ../other
  $ hg push ../other
  pushing to ../other
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  $ hg -R ../other verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 2 total revisions

Adding a changeset going extinct locally
------------------------------------------

Pull a changeset that will immediatly goes extinct (because you already have a
marker to obsolete him)
(test resolution of issue3788)

  $ hg phase --draft --force f89bcc95eba5
  $ hg phase -R ../other --draft --force f89bcc95eba5
  $ hg commit --amend -m "A''"
  $ hg --hidden --config extensions.mq= strip  --no-backup f89bcc95eba5
  $ hg pull ../other
  pulling from ../other
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

check that bundle is not affected

  $ hg bundle --hidden --rev f89bcc95eba5 --base "f89bcc95eba5^" ../f89bcc95eba5.hg
  1 changesets found
  $ hg --hidden --config extensions.mq= strip --no-backup f89bcc95eba5
  $ hg unbundle ../f89bcc95eba5.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads)
