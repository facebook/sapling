#chg-compatible

  $ enable share
  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/infinitepush/library.sh"
  $ setupcommon
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ hg init repo
  $ cd repo
  $ setupserver
  $ cd ..

Clone client
  $ hg clone ssh://user@dummy/repo client -q
  $ hg share --bookmarks client client2
  updating working directory
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client2
  $ mkcommit first
  $ hg paths
  default = ssh://user@dummy/repo

Write smth to backup state file in the shared working copy to check that
it's not read by infinitepush backup client
  $ mkdir .hg/infinitepushbackups
  $ echo 'rubbish' > .hg/infinitepushbackups/infinitepushbackupstate_f6bce706
  $ hg cloud backup
  backing up stack rooted at b75a450e74d5
  remote: pushing 1 commit:
  remote:     b75a450e74d5  first
  commitcloud: backed up 1 commit
  $ scratchbookmarks
  infinitepush/backups/test/*$TESTTMP/client/heads/b75a450e74d5a7708da8c3144fbeb4ac88694044 b75a450e74d5a7708da8c3144fbeb4ac88694044 (glob)

Make sure that backup state is saved only on the "main" repo
  $ cat .hg/infinitepushbackups/infinitepushbackupstate_f6bce706
  rubbish
  $ [ -f ../client/.hg/infinitepushbackups/infinitepushbackupstate_f6bce706 ]

Make sure that cloud check references the main repo
  $ hg cloud check -r :
  b75a450e74d5a7708da8c3144fbeb4ac88694044 backed up
  $ hg log -T '{rev}:{node} "{desc}"\n' -r 'notbackedup()'

Make another commit that is not backed up and check that too
  $ mkcommit second
  $ hg cloud check -r :
  b75a450e74d5a7708da8c3144fbeb4ac88694044 backed up
  bc64f6a267a06b03e9e0f96a6deae37ae89a832e not backed up
  $ hg log -T '{rev}:{node} "{desc}"\n' -r 'notbackedup()'
  1:bc64f6a267a06b03e9e0f96a6deae37ae89a832e "second"

