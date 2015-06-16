test sparse interaction with other extensions

  $ hg init myrepo
  $ cd myrepo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$(dirname $TESTDIR)/sparse.py
  > strip=
  > [simplecache]
  > cachedir=$TESTTMP/hgsimplecache
  > EOF

Test integration with simplecache for profile reads

  $ $PYTHON -c 'import simplecache' || exit 80
  $ printf "[include]\nfoo\n" > .hgsparse
  $ hg add .hgsparse
  $ hg commit -qm 'Add profile'
  $ hg sparse --enable-profile .hgsparse
  $ hg status --debug --config extensions.simplecache=
  falling back for value sparseprofile:.hgsparse:52fe6c0958d7d08df53bdf7ee62a261abb7f599e:v1
  set value for key sparseprofile:.hgsparse:52fe6c0958d7d08df53bdf7ee62a261abb7f599e:v1 to local
  $ hg status --debug --config extensions.simplecache=
  got value for key sparseprofile:.hgsparse:52fe6c0958d7d08df53bdf7ee62a261abb7f599e:v1 from local
