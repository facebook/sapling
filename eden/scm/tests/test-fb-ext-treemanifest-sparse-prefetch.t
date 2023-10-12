#chg-compatible
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

  $ newclientrepo client server

Checkout commits. Prefetching won't be active here, since the server doesn't
support designated nodes.

  $ hg up 'master~3'
  1 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ enable sparse
  $ hg sparse enable sparse/profile

  $ hg up 'master~2'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg up 'master'
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
