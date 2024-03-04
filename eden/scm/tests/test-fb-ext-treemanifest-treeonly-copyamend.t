#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ configure mutation-norecord
  $ . "$TESTDIR/library.sh"

Setup the server

  $ newserver master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
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
  $ clone master client
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [experimental]
  > evolution = createmarkers, allowunstable
  > [extensions]
  > amend=
  > EOF

Rename the file in a commit
  $ hg mv a/b/c/d/e/f/g/h/i/j/file a/b/c/d/e/f/g/h/i/j/file2
  * files fetched over *s (glob) (?)
  $ hg commit -m "rename"
  * files fetched over *s (glob) (?)

Amend the commit to add a new file with an empty cache, with descendantrevfastpath enabled
  $ clearcache
  $ echo more >> a/b/c/d/e/f/g/h/i/j/file3
  $ hg amend -A --config remotefilelog.debug=True --config remotefilelog.descendantrevfastpath=True
  adding a/b/c/d/e/f/g/h/i/j/file3
  * files fetched over *s (glob) (?)

Try again, disabling the descendantrevfastpath
  $ clearcache
  $ echo more >> a/b/c/d/e/f/g/h/i/j/file3
  $ hg amend -A --config remotefilelog.debug=True --config remotefilelog.descendantrevfastpath=False
  * files fetched over *s (glob) (?)
