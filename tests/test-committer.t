  $ unset HGUSER
  $ EMAIL="My Name <myname@example.com>"
  $ export EMAIL

  $ hg init test
  $ cd test
  $ touch asdf
  $ hg add asdf
  $ hg commit -d '1000000 0' -m commit-1
  $ hg tip
  changeset:   0:9426b370c206
  tag:         tip
  user:        My Name <myname@example.com>
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     commit-1
  

  $ unset EMAIL
  $ echo 1234 > asdf
  $ hg commit -d '1000000 0' -u "foo@bar.com" -m commit-1
  $ hg tip
  changeset:   1:4997f15a1b24
  tag:         tip
  user:        foo@bar.com
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     commit-1
  
  $ echo "[ui]" >> .hg/hgrc
  $ echo "username = foobar <foo@bar.com>" >> .hg/hgrc
  $ echo 12 > asdf
  $ hg commit -d '1000000 0' -m commit-1
  $ hg tip
  changeset:   2:72b8012b424e
  tag:         tip
  user:        foobar <foo@bar.com>
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     commit-1
  
  $ echo 1 > asdf
  $ hg commit -d '1000000 0' -u "foo@bar.com" -m commit-1
  $ hg tip
  changeset:   3:35ff3067bedd
  tag:         tip
  user:        foo@bar.com
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     commit-1
  
  $ echo 123 > asdf
  $ echo "[ui]" > .hg/hgrc
  $ echo "username = " >> .hg/hgrc
  $ hg commit -d '1000000 0' -m commit-1
  abort: no username supplied (see "hg help config")
  $ rm .hg/hgrc
  $ hg commit -d '1000000 0' -m commit-1 2>&1
  No username found, using '[^']*' instead

  $ echo space > asdf
  $ hg commit -d '1000000 0' -u ' ' -m commit-1
  transaction abort!
  rollback completed
  abort: empty username!

  $ true
