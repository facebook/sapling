#require gpg

Test the GPG extension

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > gpg=
  > 
  > [gpg]
  > cmd=gpg --no-permission-warning --no-secmem-warning --no-auto-check-trustdb --homedir "$TESTDIR/gpg"
  > EOF
  $ hg init r
  $ cd r
  $ echo foo > foo
  $ hg ci -Amfoo
  adding foo

  $ hg sigs

  $ HGEDITOR=cat hg sign -e 0
  signing 0:e63c23eaa88a
  Added signature for changeset e63c23eaa88a
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added .hgsigs

  $ hg sigs
  hgtest                             0:e63c23eaa88ae77967edcf4ea194d31167c478b0

  $ hg sigcheck 0
  e63c23eaa88a is signed by:
   hgtest

verify that this test has not modified the trustdb.gpg file back in
the main hg working dir
  $ md5sum.py "$TESTDIR/gpg/trustdb.gpg"
  f6b9c78c65fa9536e7512bb2ceb338ae  */gpg/trustdb.gpg (glob)

don't leak any state to next test run
  $ rm -f "$TESTDIR/gpg/random_seed"

  $ cd ..
