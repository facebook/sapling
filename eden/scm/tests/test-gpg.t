#chg-compatible

#require gpg

Test the GPG extension

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > gpg=
  > 
  > [gpg]
  > cmd=gpg --no-permission-warning --no-secmem-warning --no-auto-check-trustdb
  > EOF
  $ GNUPGHOME="$TESTTMP/gpg"; export GNUPGHOME
  $ cp -R "$TESTDIR/gpg" "$GNUPGHOME"

Start gpg-agent, which is required by GnuPG v2

#if gpg21
  $ gpg-connect-agent -q --subst /serverpid '/echo ${get serverpid}' /bye \
  > >> $DAEMON_PIDS
#endif

and migrate secret keys

#if gpg2
  $ gpg --no-permission-warning --no-secmem-warning --list-secret-keys \
  > > /dev/null 2>&1
#endif

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

  $ cd ..
