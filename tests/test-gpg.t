Test the GPG extension

  $ "$TESTDIR/hghave" gpg || exit 80
  $ if ! hg identify -q > /dev/null; then
  >     echo "skipped: not a Mercurial working dir" >&2
  >     exit 80
  > fi
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > gpg=
  > 
  > [gpg]
  > cmd=gpg --no-permission-warning --no-secmem-warning --no-auto-check-trustdb --homedir $TESTDIR/gpg
  > EOF
  $ hg init r
  $ cd r
  $ echo foo > foo
  $ hg ci -Amfoo
  adding foo

  $ hg sigs

  $ hg sign 0
  Signing 0:e63c23eaa88a

  $ hg sigs
  hgtest                             0:e63c23eaa88ae77967edcf4ea194d31167c478b0

  $ hg sigcheck 0
  e63c23eaa88a is signed by:
   hgtest

verify that this test has not modified the trustdb.gpg file back in
the main hg working dir
  $ hg --cwd $TESTDIR status gpg
