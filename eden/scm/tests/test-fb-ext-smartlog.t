#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
  $ enable smartlog
  $ readconfig <<EOF
  > [experimental]
  > graphstyle.grandparent=|
  > graphstyle.missing=|
  > EOF

Build up a repo

  $ hg init repo
  $ cd repo

Confirm smartlog doesn't error on an empty repo
  $ hg smartlog

Continue repo setup
  $ hg book master
  $ hg sl -r 'smartlog() + master'
  $ touch a1 && hg add a1 && hg ci -ma1
  $ touch a2 && hg add a2 && hg ci -ma2
  $ hg book feature1
  $ touch b && hg add b && hg ci -mb
  $ hg up -q master
  $ touch c1 && hg add c1 && hg ci -mc1
  $ touch c2 && hg add c2 && hg ci -mc2
  $ hg book feature2
  $ touch d && hg add d && hg ci -md

  $ hg debugmakepublic master
  $ hg log -G -T "{node|short} {bookmarks} {desc}" -r 'sort(:, topo)'
  @  db92053d5c83 feature2 d
  │
  o  38d85b506754 master c2
  │
  o  ec7553f7b382  c1
  │
  │ o  49cdb4091aca feature1 b
  ├─╯
  o  b68836a6e2ca  a2
  │
  o  df4fd610a3d6  a1
  

Basic test
  $ hg smartlog -T '{node|short} {bookmarks} {desc}'
  @  db92053d5c83 feature2 d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  ~

With commit info
  $ echo "hello" >c2 && hg ci --amend
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --commit-info
  @  05d10250273e feature2 d M c2
  │   A d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  ~

As a revset
  $ hg log -G -T '{node|short} {bookmarks} {desc}' -r 'smartlog()'
  @  05d10250273e feature2 d
  │
  │ o  49cdb4091aca feature1 b
  │ │
  o │  38d85b506754 master c2
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~

With --master

  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --master 'desc(a2)'
  @  05d10250273e feature2 d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  ~

Specific revs
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' -r 'desc(b)' -r 'desc(c2)' --master null
  o  49cdb4091aca feature1 b
  │
  │ o  38d85b506754 master c2
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ hg smartlog -T '{node|short} {bookmarks} {desc}' -r 'smartlog()' -r 'desc(a1)'
  @  05d10250273e feature2 d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  o  df4fd610a3d6  a1
  

Test master ordering
  $ hg debugmakepublic 49cdb4091aca

  $ hg boo -f master -r 49cdb4091aca
  $ hg smartlog -T '{node|short} {bookmarks} {desc}'
  o  49cdb4091aca feature1 master b
  │
  │ @  05d10250273e feature2 d
  │ │
  │ o  38d85b506754  c2
  │ │
  │ o  ec7553f7b382  c1
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~

Test overriding master
  $ hg debugmakepublic 38d85b506754

  $ hg boo -f master -r 38d85b506754
  $ hg smartlog -T '{node|short} {bookmarks} {desc}'
  @  05d10250273e feature2 d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ hg debugmakepublic feature1

  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --master feature1
  o  49cdb4091aca feature1 b
  │
  │ @  05d10250273e feature2 d
  │ │
  │ o  38d85b506754 master c2
  │ │
  │ o  ec7553f7b382  c1
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --config smartlog.master=feature1
  o  49cdb4091aca feature1 b
  │
  │ @  05d10250273e feature2 d
  │ │
  │ o  38d85b506754 master c2
  │ │
  │ o  ec7553f7b382  c1
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --config smartlog.master=feature2 --master feature1
  o  49cdb4091aca feature1 b
  │
  │ @  05d10250273e feature2 d
  │ │
  │ o  38d85b506754 master c2
  │ │
  │ o  ec7553f7b382  c1
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ hg debugmakepublic .

Test with weird bookmark names

  $ hg book -r 'desc(b)' foo-bar
  $ hg smartlog -r 'foo-bar + .' -T '{node|short} {bookmarks} {desc}'
  @  05d10250273e feature2 d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 foo-bar b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ hg debugmakepublic foo-bar

  $ hg smartlog --config smartlog.master=foo-bar -T '{node|short} {bookmarks} {desc}'
  o  49cdb4091aca feature1 foo-bar b
  │
  │ @  05d10250273e feature2 d
  │ │
  │ o  38d85b506754 master c2
  │ │
  │ o  ec7553f7b382  c1
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~
  $ hg smartlog --config smartlog.master=xxxx -T '{node|short} {bookmarks} {desc}'
  abort: unknown revision 'xxxx'!
  [255]

Test with two unrelated histories
  $ hg goto null
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  (leaving bookmark feature2)
  $ touch u1 && hg add u1 && hg ci -mu1
  $ touch u2 && hg add u2 && hg ci -mu2

  $ hg smartlog  -T '{node|short} {bookmarks} {desc}'
  @  806aaef35296  u2
  │
  o  8749dc393678  u1
  
  o  05d10250273e feature2 d
  │
  o  38d85b506754 master c2
  │
  o  ec7553f7b382  c1
  │
  │ o  49cdb4091aca feature1 foo-bar b
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~


A draft stack at the top
  $ cd ..
  $ hg init repo2
  $ cd repo2
  $ hg debugbuilddag '+4'
  $ hg bookmark curr
  $ hg bookmark master -r 'desc(r1)'
  $ hg debugmakepublic -r 'desc(r1)'
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --all
  o  2dc09a01254d  r3
  │
  o  01241442b3c2  r2
  │
  o  66f7d451a68b master r1
  │
  ~
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
    o  2dc09a01254d  r3
    │
    o  01241442b3c2  r2
  ╭─╯
  o  66f7d451a68b master r1
  │
  ~

Different number of lines per node

  $ hg smartlog -T '{node|short}\n{bookmarks}\n{desc}\n{author}\n{date|isodate}\n' --all --config smartlog.indentnonpublic=1
    o  2dc09a01254d
    │
    │  r3
    │  debugbuilddag
    │  1970-01-01 00:00 +0000
    o  01241442b3c2
  ╭─╯
  │    r2
  │    debugbuilddag
  │    1970-01-01 00:00 +0000
  o  66f7d451a68b
  │  master
  ~  r1
     debugbuilddag
     1970-01-01 00:00 +0000

Add other draft stacks
  $ hg up 'desc(r1)' -q
  $ echo 1 > a
  $ hg ci -A a -m a -q
  $ echo 2 >> a
  $ hg ci -A a -m a -q
  $ hg up 'desc(r2)' -q
  $ echo 2 > b
  $ hg ci -A b -m b -q
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
    @  401cd6213b51  b
    │
    │ o  2dc09a01254d  r3
    ├─╯
    o  01241442b3c2  r2
  ╭─╯
  │ o  a60fccdcd9e9  a
  │ │
  │ o  8d92afe5abfd  a
  ├─╯
  o  66f7d451a68b master r1
  │
  ~

Limit by threshold

  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.max-commit-threshold=2
  smartlog: too many (6) commits, not rendering all of them
  (consider running 'hg doctor' to hide unrelated commits)
  @  401cd6213b51  b
  ╷
  ╷ o  a60fccdcd9e9  a
  ╭─╯
  ╷ o  2dc09a01254d  r3
  ╭─╯
  o  66f7d451a68b master r1
  │
  ~

Recent arg select days correctly
  $ echo 1 >> b
  $ myday=`hg debugsh -c 'import time; ui.write(str(int(time.time()) - 24 * 3600 * 20))'`
  $ hg commit --date "$myday 0" -m test2
  $ hg goto 'desc(r0)' -q
  $ hg log -Gr 'smartlog(master="master", heads=((date(-15) & draft()) + .))' -T '{node|short} {bookmarks} {desc}'
  o  66f7d451a68b master r1
  │
  @  1ea73414a91b  r0
  

  $ hg log -Gr 'smartlog((date(-25) & draft()) + .)' -T '{bookmarks} {desc}'
  o   test2
  │
  o   b
  │
  o   r2
  │
  o  master r1
  │
  @   r0
  
Make sure public commits that are descendants of master are not drawn
  $ cd ..
  $ hg init repo3
  $ cd repo3
  $ hg debugbuilddag '+5'
  $ hg bookmark master -r 'desc(r1)'
  $ hg debugmakepublic -r 'desc(r1)'
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
    o  bebd167eb94d  r4
    │
    o  2dc09a01254d  r3
    │
    o  01241442b3c2  r2
  ╭─╯
  o  66f7d451a68b master r1
  │
  ~
  $ hg debugmakepublic 'desc(r3)'
  $ hg up -q 'desc(r4)'
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
    @  bebd167eb94d  r4
  ╭─╯
  o  2dc09a01254d  r3
  ╷
  o  66f7d451a68b master r1
  │
  ~
  $ hg debugmakepublic 'desc(r4)'
  $ hg smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
  @  bebd167eb94d  r4
  ╷
  o  66f7d451a68b master r1
  │
  ~

