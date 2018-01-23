Share works with blackbox enabled:

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > blackbox =
  > share =
  > EOF

  $ hg init a
  $ hg share a b
  updating working directory
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd b
  $ hg unshare
