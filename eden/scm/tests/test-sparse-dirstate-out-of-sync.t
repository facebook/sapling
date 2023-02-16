#chg-compatible
#debugruntest-compatible
  $ setconfig workingcopy.ruststatus=False
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

  $ hg status --config status.use-rust=true --config workingcopy.use-rust=true

# XXX fixme - Python status should skip file not in sparse profile.
  $ hg status --config status.use-rust=false --config workingcopy.use-rust=false
  R a

