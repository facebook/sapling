#chg-compatible

  $ enable amend
  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setconfig extensions.commitcloud=

  $ setupcommon

  $ hginit server
  $ cd server
  $ setupserver
  $ setconfig remotefilelog.server=true

  $ touch base
  $ hg commit -Aqm base
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/server shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *.*s (glob)
  $ cd shallow

Test pushing of specific sets of commits
  $ drawdag <<'EOS'
  >  B  C          
  >  |  |          
  >  A1 A2   D1 D2 D3  E1  E2
  >    \|      \|  |    \ /
  >     .       .  .     .
  >                # amend: A1 -> A2
  >                # amend: D1 -> D2 -> D3
  >                # rebase: E1 -> E2
  > EOS
  $ hg book -r $E1 pinnedvisible --hidden
  $ hg up $D2 -q --hidden

Check backing up top stack commit and mid commit
  $ hg cloud check -r $A1 -r $D2 -r $E1
  * not backed up (glob)
  * not backed up (glob)
  * not backed up (glob)

  $ hg cloud backup --traceback
  backing up stack rooted at 64164d1e0f82
  remote: pushing 2 commits:
  remote:     64164d1e0f82  A1
  remote:     796f1f48de85  B
  backing up stack rooted at d0d71d09c927
  remote: pushing 2 commits:
  remote:     d0d71d09c927  A2
  remote:     daeeb2f180d6  C
  backing up stack rooted at d79a807cba78
  remote: pushing 1 commit:
  remote:     d79a807cba78  D2
  backing up stack rooted at eccc11f58a56
  remote: pushing 1 commit:
  remote:     eccc11f58a56  D3
  backing up stack rooted at 42952ab62cec
  remote: pushing 1 commit:
  remote:     42952ab62cec  E1
  backing up stack rooted at 4903fdffd9c6
  remote: pushing 1 commit:
  remote:     4903fdffd9c6  E2
  commitcloud: backed up 8 commits

  $ hg cloud check -r $A1 -r $D2 -r $E1
  64164d1e0f82f6a670c84728b83061df1b126b5c backed up
  d79a807cba78db45ec042b74da65ebfd6d58eadd backed up
  42952ab62cecf85e36eaab6965b6bf3f5e3e9fe1 backed up
  $ hg cloud check -r $D1 --hidden
  7c8a43610cd6d316f9bec941fa2677e5c7a90bf5 not backed up

