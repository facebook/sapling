#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure modern
  $ setconfig remotefilelog.debug=True

  $ newserver server

  $ mkdir sparse
  $ cat > sparse/profile <<EOF
  > path:sparse/
  > EOF
  $ hg commit -Aqm 'initial'

  $ mkdir foo bar bar/bar bar/bar/bar
  $ touch foo/123 bar/bar/bar/456
  $ hg commit -Aqm 'add files'

  $ cat >> sparse/profile <<EOF
  > # some comment
  > EOF
  $ hg commit -Aqm 'modify sparse profile'

  $ touch foo/456
  $ hg commit -Aqm 'add more files'

  $ hg bookmark -r tip master

  $ cd ..

  $ clone server client --noupdate

Checkout commits. Prefetching won't be active here, since the server doesn't
support designated nodes.

  $ cd client
  $ hg up 'master~3'
  fetching tree '' 4ccb43944747fdc11a890fcae40e0bc0ac6732da
  1 trees fetched over 0.00s
  fetching tree 'sparse' 24c75048c8e4debd244f3d2a15ff6442906f6702
  1 trees fetched over 0.00s
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ enable sparse
  $ hg sparse enable sparse/profile

  $ hg up 'master~2'
  fetching tree '' 4bdc11054000cc0fbdbafe300c7589072b5426ca
  1 trees fetched over 0.00s
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg up 'master'
  fetching tree '' ad42fc7bd685adac2344311e2330b67b14e2beaf
  1 trees fetched over 0.00s
  fetching tree 'sparse' e738d530b4579275fc0b50efbe7204cb7b4d8266
  1 trees fetched over 0.00s
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Check that we can create some commits, and that nothing breaks even if the
server does not know about our root manifest.

  $ hg book client

  $ cat >> sparse/profile <<EOF
  > # more comment
  > EOF
  $ hg commit -Aqm 'modify sparse profile again'

  $ hg up 'client~1'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark client)

  $ hg up 'client'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark client)
