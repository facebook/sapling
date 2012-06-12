  $ unset HGUSER
  $ EMAIL="My Name <myname@example.com>"
  $ export EMAIL

  $ hg init test
  $ cd test
  $ touch asdf
  $ hg add asdf
  $ hg commit -m commit-1
  $ hg tip
  changeset:   0:53f268a58230
  tag:         tip
  user:        My Name <myname@example.com>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  

  $ unset EMAIL
  $ echo 1234 > asdf
  $ hg commit -u "foo@bar.com" -m commit-1
  $ hg tip
  changeset:   1:3871b2a9e9bf
  tag:         tip
  user:        foo@bar.com
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  
  $ echo "[ui]" >> .hg/hgrc
  $ echo "username = foobar <foo@bar.com>" >> .hg/hgrc
  $ echo 12 > asdf
  $ hg commit -m commit-1
  $ hg tip
  changeset:   2:8eeac6695c1c
  tag:         tip
  user:        foobar <foo@bar.com>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  
  $ echo 1 > asdf
  $ hg commit -u "foo@bar.com" -m commit-1
  $ hg tip
  changeset:   3:957606a725e4
  tag:         tip
  user:        foo@bar.com
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  
  $ echo 123 > asdf
  $ echo "[ui]" > .hg/hgrc
  $ echo "username = " >> .hg/hgrc
  $ hg commit -m commit-1
  abort: no username supplied (see "hg help config")
  [255]
  $ rm .hg/hgrc
  $ hg commit -m commit-1 2>&1
  no username found, using '[^']*' instead (re)

  $ echo space > asdf
  $ hg commit -u ' ' -m commit-1
  transaction abort!
  rollback completed
  abort: empty username!
  [255]

  $ cd ..
