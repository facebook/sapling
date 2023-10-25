#debugruntest-compatible

  $ eagerepo

  $ hg init test
  $ cd test
  $ touch asdf
  $ hg add asdf
  $ HGUSER="My Name <myname@example.com>" hg commit -m commit-1
  $ hg tip
  commit:      53f268a58230
  user:        My Name <myname@example.com>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  

  $ echo 1234 > asdf
  $ hg commit -u "foo@bar.com" -m commit-1
  $ hg tip
  commit:      3871b2a9e9bf
  user:        foo@bar.com
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  
  $ unset HGUSER
  $ echo "[ui]" >> .hg/hgrc
  $ echo "username = foobar <foo@bar.com>" >> .hg/hgrc
  $ echo 12 > asdf
  $ hg commit -m commit-1
  $ hg tip
  commit:      8eeac6695c1c
  user:        foobar <foo@bar.com>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  
  $ echo 1 > asdf
  $ hg commit -u "foo@bar.com" -m commit-1
  $ hg tip
  commit:      957606a725e4
  user:        foo@bar.com
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  
  $ echo 123 > asdf
  $ echo "[ui]" > .hg/hgrc
  $ echo "username = " >> .hg/hgrc
  $ hg commit -m commit-1
  abort: no username supplied
  (use `hg config --user ui.username "First Last <me@example.com>"` to set your username)
  [255]

# test alternate config var

  $ echo 1234 > asdf
  $ echo "[ui]" > .hg/hgrc
  $ echo "user = Foo Bar II <foo2@bar.com>" >> .hg/hgrc
  $ hg commit -m commit-1
  $ hg tip
  commit:      6f24bfb4c617
  user:        Foo Bar II <foo2@bar.com>
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-1
  
# test no .hg/hgrc (uses generated non-interactive username)

  $ echo space > asdf
  $ echo '%unset username' >> .hg/hgrc
  $ echo '%unset user' >> .hg/hgrc
  $ HGPLAIN=1 hg commit -m commit-1
  no username found, using '[^']*' instead (re)

  $ echo space2 > asdf
  $ hg commit -u ' ' -m commit-1
  abort: empty username!
  [255]

# test prompt username and it gets stored

  $ cat > .hg/hgrc <<EOF
  > [ui]
  > askusername = True
  > EOF

  $ echo 12345 > asdf

  $ hg commit --config ui.interactive=True -m ask <<EOF
  > Asked User <ask@example.com>
  > EOF
  enter a commit username: Asked User <ask@example.com>
  $ hg log -r. -T '{author}\n'
  Asked User <ask@example.com>

  $ hg config ui.username --debug
  $TESTTMP/.hgrc:*: Asked User <ask@example.com> (glob)

# don't add tests here, previous test is unstable

  $ cd ..
