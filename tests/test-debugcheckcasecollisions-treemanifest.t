The ordering and format of case collisions detected using treemanifest is
different, so this is a different test script.

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF

  $ hgcloneshallow ssh://user@dummy/master client -q
