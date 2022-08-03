#chg-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure mutation-norecord
  $ . "$TESTDIR/library.sh"

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

Make local commits on the server for a file in a deep directory with a long
history, where the new file content is introduced on a separate branch each
time.
  $ mkdir -p a/b/c/d/e/f/g/h/i/j
  $ echo "base" > a/b/c/d/e/f/g/h/i/j/file
  $ hg commit -qAm "base"
  $ for i in 1 2 3 4 5 6 7 8 9 10 11 12
  > do
  >   echo $i >> a/b/c/d/e/f/g/h/i/j/file
  >   echo $i >> a/b/c/d/e/f/g/h/i/otherfile$i
  >   hg commit -qAm "commit $i branch"
  >   hg up -q ".^"
  >   echo $i >> a/b/c/d/e/f/g/h/i/j/file
  >   echo $i >> a/b/c/d/e/f/g/h/i/otherfile$i
  >   hg commit -qAm "commit $i"
  > done

  $ hg log -G -r 'all()' -T '{desc}'
  @  commit 12
  │
  │ o  commit 12 branch
  ├─╯
  o  commit 11
  │
  │ o  commit 11 branch
  ├─╯
  o  commit 10
  │
  │ o  commit 10 branch
  ├─╯
  o  commit 9
  │
  │ o  commit 9 branch
  ├─╯
  o  commit 8
  │
  │ o  commit 8 branch
  ├─╯
  o  commit 7
  │
  │ o  commit 7 branch
  ├─╯
  o  commit 6
  │
  │ o  commit 6 branch
  ├─╯
  o  commit 5
  │
  │ o  commit 5 branch
  ├─╯
  o  commit 4
  │
  │ o  commit 4 branch
  ├─╯
  o  commit 3
  │
  │ o  commit 3 branch
  ├─╯
  o  commit 2
  │
  │ o  commit 2 branch
  ├─╯
  o  commit 1
  │
  │ o  commit 1 branch
  ├─╯
  o  base
  
Create a client
  $ hgcloneshallow ssh://user@dummy/master client -q
  13 files fetched over *s (glob) (?)
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  1 trees fetched over 0.00s
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution = createmarkers, allowunstable
  > [extensions]
  > amend=
  > [treemanifest]
  > sendtrees=True
  > [remotefilelog]
  > reponame=treeonlyrepo
  > EOF

Rename the file in a commit
  $ hg mv a/b/c/d/e/f/g/h/i/j/file a/b/c/d/e/f/g/h/i/j/file2
  * files fetched over *s (glob) (?)
  fetching tree '' efa8fa4352b919302f90e85924e691a632d6bea0
  1 trees fetched over 0.00s
  fetching tree 'a' da4a8c7aed08ac15737161f1141f62c8bf5863ff
  1 trees fetched over 0.00s
  fetching tree 'a/b' 7bce920d6eb775219b166d3ff5ed179beb911262
  1 trees fetched over 0.00s
  fetching tree 'a/b/c' d3b274e3a1a3eb3b2e40649edf5fa760ebe4244a
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d' 9ebcd13da6585be7f4a193758ab658900177485f
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e' e813c621f9cb5854cf135aadf40e583aa82cde17
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f' ac2756cfc47ad4292e74abdf5e2f64e4a5a87150
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g' b549e4b73c3f8e5e58ebf82ae4da98b29303af07
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h' 44efe33f66d2e94e5d0ed0ecf71a5261c886fddf
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h/i' 368f5aab3f700081ac9a8bc4e4ae1058aa9b8140
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 22ac44092b0a117fbfeabff7839cd97964ebc4ea
  1 trees fetched over 0.00s
  $ hg commit -m "rename"
  * files fetched over *s (glob) (?)

Amend the commit to add a new file with an empty cache, with descendantrevfastpath enabled
  $ clearcache
  $ echo more >> a/b/c/d/e/f/g/h/i/j/file3
  $ hg amend -A --config remotefilelog.debug=True --config remotefilelog.descendantrevfastpath=True
  adding a/b/c/d/e/f/g/h/i/j/file3
  fetching tree '' efa8fa4352b919302f90e85924e691a632d6bea0
  * files fetched over *s (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'a' da4a8c7aed08ac15737161f1141f62c8bf5863ff
  1 trees fetched over 0.00s
  fetching tree 'a/b' 7bce920d6eb775219b166d3ff5ed179beb911262
  1 trees fetched over 0.00s
  fetching tree 'a/b/c' d3b274e3a1a3eb3b2e40649edf5fa760ebe4244a
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d' 9ebcd13da6585be7f4a193758ab658900177485f
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e' e813c621f9cb5854cf135aadf40e583aa82cde17
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f' ac2756cfc47ad4292e74abdf5e2f64e4a5a87150
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g' b549e4b73c3f8e5e58ebf82ae4da98b29303af07
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h' 44efe33f66d2e94e5d0ed0ecf71a5261c886fddf
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h/i' 368f5aab3f700081ac9a8bc4e4ae1058aa9b8140
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 22ac44092b0a117fbfeabff7839cd97964ebc4ea
  1 trees fetched over 0.00s

Try again, disabling the descendantrevfastpath
  $ clearcache
  $ echo more >> a/b/c/d/e/f/g/h/i/j/file3
  $ hg amend -A --config remotefilelog.debug=True --config remotefilelog.descendantrevfastpath=False
  fetching tree '' efa8fa4352b919302f90e85924e691a632d6bea0
  * files fetched over *s (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'a' da4a8c7aed08ac15737161f1141f62c8bf5863ff
  1 trees fetched over 0.00s
  fetching tree 'a/b' 7bce920d6eb775219b166d3ff5ed179beb911262
  1 trees fetched over 0.00s
  fetching tree 'a/b/c' d3b274e3a1a3eb3b2e40649edf5fa760ebe4244a
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d' 9ebcd13da6585be7f4a193758ab658900177485f
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e' e813c621f9cb5854cf135aadf40e583aa82cde17
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f' ac2756cfc47ad4292e74abdf5e2f64e4a5a87150
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g' b549e4b73c3f8e5e58ebf82ae4da98b29303af07
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h' 44efe33f66d2e94e5d0ed0ecf71a5261c886fddf
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h/i' 368f5aab3f700081ac9a8bc4e4ae1058aa9b8140
  1 trees fetched over 0.00s
  fetching tree 'a/b/c/d/e/f/g/h/i/j' 22ac44092b0a117fbfeabff7839cd97964ebc4ea
  1 trees fetched over 0.00s
