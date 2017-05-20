A repo with unknown revlogv2 requirement string cannot be opened

  $ hg init invalidreq
  $ cd invalidreq
  $ echo exp-revlogv2.unknown >> .hg/requires
  $ hg log
  abort: repository requires features unknown to this Mercurial: exp-revlogv2.unknown!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ cd ..

Can create and open repo with revlog v2 requirement

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > revlogv2 = enable-unstable-format-and-corrupt-my-data
  > EOF

  $ hg init empty-repo
  $ cd empty-repo
  $ cat .hg/requires
  dotencode
  exp-revlogv2.0
  fncache
  store

  $ hg log

Unknown flags to revlog are rejected

  >>> with open('.hg/store/00changelog.i', 'wb') as fh:
  ...     fh.write('\x00\x04\xde\xad')

  $ hg log
  abort: unknown flags (0x04) in version 57005 revlog 00changelog.i!
  [255]

  $ cd ..

Writing a simple revlog v2 works

  $ hg init simple
  $ cd simple
  $ touch foo
  $ hg -q commit -A -m initial

  $ hg log
  changeset:   0:96ee1d7354c4
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial
  
Header written as expected (changelog always disables generaldelta)

  $ f --hexdump --bytes 4 .hg/store/00changelog.i
  .hg/store/00changelog.i:
  0000: 00 01 de ad                                     |....|

  $ f --hexdump --bytes 4 .hg/store/data/foo.i
  .hg/store/data/foo.i:
  0000: 00 03 de ad                                     |....|
