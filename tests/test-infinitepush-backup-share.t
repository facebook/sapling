  $ . "$TESTDIR/library.sh"
  $ . "$TESTDIR/library-infinitepush.sh"
  $ setupcommon
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "$1"
  > }
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > share=
  > EOF
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
  $ echo 'rubbish' > .hg/infinitepushbackupstate
  $ hg pushbackup
  starting backup .* (re)
  searching for changes
  remote: pushing 1 commit:
  remote:     b75a450e74d5  first
  finished in \d+\.(\d+)? seconds (re)
  $ scratchbookmarks
  infinitepush/backups/test/*$TESTTMP/client/heads/b75a450e74d5a7708da8c3144fbeb4ac88694044 b75a450e74d5a7708da8c3144fbeb4ac88694044 (glob)

Make sure that backup state is saved only on the "main" repo
  $ cat .hg/infinitepushbackupstate
  rubbish
  $ [ -f ../client/.hg/infinitepushbackupstate ]
