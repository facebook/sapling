  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > username = nobody <no.reply@fb.com>
  > [extensions]
  > strip =
  > EOF
  $ alias commit='hg commit -d "0 0" -A -m'
  $ alias log='hg log -G -T "{desc} [{phase}:{node|short}]"'

Set up server repository

  $ hg init server
  $ cd server
  $ echo foo > a
  $ echo foo > b
  $ commit 'initial'
  adding a
  adding b

Set up client repository

  $ cd ..
  $ hg clone ssh://user@dummy/server client -q
  $ cd client
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase = $TESTDIR/../pushrebase.py" >> .hg/hgrc

Without server extension

  $ cd ../server
  $ echo 'bar' > a
  $ commit 'a => bar'

  $ cd ../client
  $ echo 'bar' > b
  $ commit 'b => bar'
  $ echo 'baz' > b
  $ commit 'b => baz'
  $ hg push
  pushing to ssh://user@dummy/server
  searching for changes
  remote has heads on branch 'default' that are not known locally: add0c792bfce
  abort: push creates new remote head 2e6d0db3b0dd!
  (pull and merge or see "hg help push" for details about pushing new heads)
  [255]

  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: bundle2 needs to be enabled on client
  [255]

  $ echo "[experimental]" >> .hg/hgrc
  $ echo "bundle2-exp = True" >> .hg/hgrc
  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: bundle2 needs to be enabled on server
  [255]

  $ echo "[experimental]" >> ../server/.hg/hgrc
  $ echo "bundle2-exp = True" >> ../server/.hg/hgrc
  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: no server support for 'b2x:rebase'
  [255]

Stack of non-conflicting commits should be accepted

  $ cd ../server
  $ echo "[extensions]" >> .hg/hgrc
  $ echo "pushrebase = $TESTDIR/../pushrebase.py" >> .hg/hgrc
  $ log
  @  a => bar [draft:add0c792bfce]
  |
  o  initial [draft:2bb9d20e471c]
  

  $ cd ../client
  $ log
  @  b => baz [draft:2e6d0db3b0dd]
  |
  o  b => bar [draft:7585d2e4bf9a]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes

  $ cd ../server
  $ hg update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log
  @  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

  $ cd ../client
  $ hg strip 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/7585d2e4bf9a-backup.hg
  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files
  (run 'hg update' to get a working copy)
  $ hg update default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Regular commits should go through without changing hash

  $ cd ../client
  $ echo '[experimental]' >> .hg/hgrc
  $ echo 'bundle2.pushback = True' >> .hg/hgrc

  $ echo 'quux' > b
  $ commit 'b => quux'
  $ log -r tip
  @  b => quux [draft:137b1b6ef903]
  |

  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes

  $ cd ../server
  $ hg update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ log
  @  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

Stack with conflict in tail should abort

  $ cd ../server
  $ echo 'baz' > a
  $ commit 'a => baz'

  $ cd ../client
  $ echo 'quux' > a
  $ commit 'a => quux'
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: conflicting changes in ['a']
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/17000cb52871-backup.hg
  $ cd ../server
  $ log
  @  a => baz [draft:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

Stack with conflict in head should abort

  $ cd ../client
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  $ echo 'quux' > a
  $ commit 'a => quux'
  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes
  abort: conflicting changes in ['a']
  [255]

  $ hg strip 5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/6e1d0b2f8180-backup.hg

  $ cd ../server
  $ log
  @  a => baz [draft:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

With evolution enabled, should set obsolescence markers

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "rebase =" >> $HGRCPATH
  $ echo "evolve =" >> $HGRCPATH

  $ cd ../client
  $ echo 'foofoo' > b
  $ commit 'b => foofoo'
  $ echo 'foobar' > b
  $ commit 'b => foobar'
  $ log
  @  b => foobar [draft:a754b7172e58]
  |
  o  b => foofoo [draft:6e1d0b2f8180]
  |
  o  b => quux [draft:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
  $ hg push --onto default
  pushing to ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 1 changes to 2 files (+1 heads)
  2 new obsolescence markers

  $ hg pull
  pulling from ssh://user@dummy/server
  searching for changes
  no changes found
  working directory parent is obsolete!

  $ hg evolve
  update:[9] b => foobar
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  working directory is now at b423e42e5548

  $ log
  @  b => foobar [public:b423e42e5548]
  |
  o  b => foofoo [public:5402bb2493c7]
  |
  o  a => baz [public:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  

  $ cd ../server
  $ log
  o  b => foobar [public:b423e42e5548]
  |
  o  b => foofoo [public:5402bb2493c7]
  |
  @  a => baz [public:ddd9491cc0b4]
  |
  o  b => quux [public:137b1b6ef903]
  |
  o  b => baz [public:7ba922f02e46]
  |
  o  b => bar [public:fe66d1686ec2]
  |
  o  a => bar [public:add0c792bfce]
  |
  o  initial [public:2bb9d20e471c]
  
TODO: test pushing bookmarks
TODO: test hooks getting called
