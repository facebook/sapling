#debugruntest-compatible

  $ configure modernclient

  $ enable sparse
  $ newclientrepo myrepo
  $ touch a
  $ hg commit -Aqm a
  $ hg rm a
  $ cat > .hg/sparse <<EOF
  > [exclude]
  > a
  > EOF

We should filter out "a" since it isn't included in the sparse profile.
  $ hg status
