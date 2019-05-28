  $ setconfig extensions.treemanifest=!

Setup


  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    echo "add $1" > msg
  >    echo "" >> msg
  >    hg ci -l msg
  > }

Check that remotename changes trigger caching
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > remotenames=
  > fastmanifest=
  > [fastmanifest]
  > cacheonchange=True
  > cacheonchangebackground=False
  > [remotenames]
  > rename.default=remote
  > EOF

  $ hg init server
  $ cd server
  $ mkcommit a
  $ mkcommit b
  $ mkcommit c
  $ hg book master -r ".^"
  $ cd ..
  $ hg clone server client -q
  $ cd server
  $ hg book master -r "." -f
  $ cd ../client
  $ hg log -r "fastmanifesttocache()"
  changeset:   1:7c3bad9141dc
  bookmark:    remote/master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add b
  
  $ hg debugcache -a
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7 (size 184 bytes)
  cache size is: 184 bytes
  number of entries is: 1
  Most relevant cache entries appear first
  ================================================================================
  manifest node                           |revs
  a539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7|1
  $ hg debugcachemanifest --debug --pruneall
  [FM] caching revset: [], pruneall(True), list(False)
  [FM] removing cached manifest fasta539ce0c1a22b0ecf34498f9f5ce8ea56df9ecb7
  $ hg pull -r master
  pulling from $TESTTMP/server (glob)
  no changes found
  $ hg log -r remote/master
  changeset:   2:4538525df7e2
  tag:         tip
  bookmark:    remote/master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add c
  
  $ hg debugcachemanifest --debug --list
  [FM] caching revset: [], pruneall(False), list(True)
  faste3738bf5439958f89499a656982023aba57b076e (size 232 bytes)
  cache size is: 232 bytes
  number of entries is: 1
  Most relevant cache entries appear first
  ================================================================================
  manifest node                           |revs
  e3738bf5439958f89499a656982023aba57b076e|2
