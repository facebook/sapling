
  $ cat << EOF >> $HGRCPATH
  > [format]
  > usegeneraldelta=yes
  > EOF

bundle w/o type option

  $ hg init t1
  $ hg init t2
  $ cd t1
  $ echo blablablablabla > file.txt
  $ hg ci -Ama
  adding file.txt
  $ hg log | grep summary
  summary:     a
  $ hg bundle ../b1 ../t2
  searching for changes
  1 changesets found

  $ cd ../t2
  $ hg pull ../b1
  pulling from ../b1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log | grep summary
  summary:     a
  $ cd ..

test bundle types

  $ testbundle() {
  >   echo % test bundle type $1
  >   hg init t$1
  >   cd t1
  >   hg bundle -t $1 ../b$1 ../t$1
  >   f -q -B6 -D ../b$1; echo
  >   cd ../t$1
  >   hg debugbundle ../b$1
  >   hg debugbundle --spec ../b$1
  >   echo
  >   cd ..
  > }

  $ for t in "None" "bzip2" "gzip" "none-v2" "v2" "v1" "gzip-v1"; do
  >   testbundle $t
  > done
  % test bundle type None
  searching for changes
  1 changesets found
  HG20\x00\x00 (esc)
  Stream params: {}
  changegroup -- "sortdict([('version', '02'), ('nbchanges', '1')])"
      c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  none-v2
  
  % test bundle type bzip2
  searching for changes
  1 changesets found
  HG20\x00\x00 (esc)
  Stream params: sortdict([('Compression', 'BZ')])
  changegroup -- "sortdict([('version', '02'), ('nbchanges', '1')])"
      c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  bzip2-v2
  
  % test bundle type gzip
  searching for changes
  1 changesets found
  HG20\x00\x00 (esc)
  Stream params: sortdict([('Compression', 'GZ')])
  changegroup -- "sortdict([('version', '02'), ('nbchanges', '1')])"
      c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  gzip-v2
  
  % test bundle type none-v2
  searching for changes
  1 changesets found
  HG20\x00\x00 (esc)
  Stream params: {}
  changegroup -- "sortdict([('version', '02'), ('nbchanges', '1')])"
      c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  none-v2
  
  % test bundle type v2
  searching for changes
  1 changesets found
  HG20\x00\x00 (esc)
  Stream params: sortdict([('Compression', 'BZ')])
  changegroup -- "sortdict([('version', '02'), ('nbchanges', '1')])"
      c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  bzip2-v2
  
  % test bundle type v1
  searching for changes
  1 changesets found
  HG10BZ
  c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  bzip2-v1
  
  % test bundle type gzip-v1
  searching for changes
  1 changesets found
  HG10GZ
  c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  gzip-v1
  
#if zstd

  $ for t in "zstd" "zstd-v2"; do
  >   testbundle $t
  > done
  % test bundle type zstd
  searching for changes
  1 changesets found
  HG20\x00\x00 (esc)
  Stream params: sortdict([('Compression', 'ZS')])
  changegroup -- "sortdict([('version', '02'), ('nbchanges', '1')])"
      c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  zstd-v2
  
  % test bundle type zstd-v2
  searching for changes
  1 changesets found
  HG20\x00\x00 (esc)
  Stream params: sortdict([('Compression', 'ZS')])
  changegroup -- "sortdict([('version', '02'), ('nbchanges', '1')])"
      c35a0f9217e65d1fdb90c936ffa7dbe679f83ddf
  zstd-v2
  
#else

zstd is a valid engine but isn't available

  $ hg -R t1 bundle -a -t zstd irrelevant.hg
  abort: compression engine zstd could not be loaded
  [255]

#endif

test garbage file

  $ echo garbage > bgarbage
  $ hg init tgarbage
  $ cd tgarbage
  $ hg pull ../bgarbage
  pulling from ../bgarbage
  abort: ../bgarbage: not a Mercurial bundle
  [255]
  $ cd ..

test invalid bundle type

  $ cd t1
  $ hg bundle -a -t garbage ../bgarbage
  abort: garbage is not a recognized bundle specification
  (see 'hg help bundle' for supported values for --type)
  [255]
  $ cd ..
