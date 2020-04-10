#chg-compatible

#require serve

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > schemes=
  > 
  > [schemes]
  > z = file:\$PWD/
  > EOF
  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Am initial
  adding a

check that paths are expanded

  $ PWD=`pwd` hg incoming z://
  comparing with z://
  searching for changes
  no changes found

check that debugexpandscheme outputs the canonical form

  $ hg debugexpandscheme bb://user/repo
  https://bitbucket.org/user/repo

expanding an unknown scheme emits the input

  $ hg debugexpandscheme foobar://this/that
  foobar://this/that

expanding a canonical URL emits the input

  $ hg debugexpandscheme https://bitbucket.org/user/repo
  https://bitbucket.org/user/repo
