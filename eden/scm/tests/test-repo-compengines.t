A new repository uses zlib storage, which doesn't need a requirement

  $ hg init default
  $ cd default
  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  revlogv1
  store
  treestate

  $ touch foo
  $ hg -q commit -A -m 'initial commit with a lot of repeated repeated repeated text to trigger compression'
  $ hg debugrevlog -c | grep 0x78
      0x78 (x)  :   1 (100.00%)
      0x78 (x)  : 110 (100.00%)

  $ cd ..

Unknown compression engine to format.compression aborts

  $ hg --config experimental.format.compression=unknown init unknown
  abort: compression engine unknown defined by experimental.format.compression not available
  (run "hg debuginstall" to list available compression engines)
  [255]

A requirement specifying an unknown compression engine results in bail

  $ hg init unknownrequirement
  $ cd unknownrequirement
  $ echo exp-compression-unknown >> .hg/requires
  $ hg log
  abort: repository requires features unknown to this Mercurial: exp-compression-unknown!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

  $ cd ..

#if zstd

  $ hg --config experimental.format.compression=zstd init zstd
  $ cd zstd
  $ cat .hg/requires
  dotencode
  exp-compression-zstd
  fncache
  generaldelta
  revlogv1
  store
  treestate

  $ touch foo
  $ hg -q commit -A -m 'initial commit with a lot of repeated repeated repeated text'

  $ hg debugrevlog -c | grep 0x28
      0x28      :  1 (100.00%)
      0x28      : 98 (100.00%)

  $ cd ..

Specifying a new format.compression on an existing repo won't introduce data
with that engine or a requirement

  $ cd default
  $ touch bar
  $ hg --config experimental.format.compression=zstd -q commit -A -m 'add bar with a lot of repeated repeated repeated text'

  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  revlogv1
  store
  treestate

  $ hg debugrevlog -c | grep 0x78
      0x78 (x)  :   2 (100.00%)
      0x78 (x)  : 199 (100.00%)

#endif
