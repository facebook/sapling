#require no-eden

  $ enable smartlog
  $ readconfig <<EOF
  > [experimental]
  > graphstyle.grandparent=|
  > graphstyle.missing=|
  > EOF

Build up a repo

  $ sl init repo
  $ cd repo

Confirm smartlog doesn't error on an empty repo
  $ sl smartlog

Continue repo setup
  $ sl book master
  $ sl sl -r 'smartlog() + master'
  $ touch a1 && sl add a1 && sl ci -ma1
  $ touch a2 && sl add a2 && sl ci -ma2
  $ sl book feature1
  $ touch b && sl add b && sl ci -mb
  $ sl up -q master
  $ touch c1 && sl add c1 && sl ci -mc1
  $ touch c2 && sl add c2 && sl ci -mc2
  $ sl book feature2
  $ touch d && sl add d && sl ci -md

  $ sl debugmakepublic master
  $ sl log -G -T "{node|short} {bookmarks} {desc}" -r 'sort(:, topo)'
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
  $ sl smartlog -T '{node|short} {bookmarks} {desc}'
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
  $ echo "hello" >c2 && sl ci --amend
  $ USER=test SL_CONFIG_PATH="$SL_CONFIG_PATH;fb=static" sl smartlog -T '{sl}' --commit-info --config extensions.commitcloud=!
    @  05d102502  Today at 00:00  test  feature2*
  ╭─╯  d
  │
  │     M c2
  │     A d
  │
  o  38d85b506  Today at 00:00  test  master public/38d85b50675416788125247211034703776317bd
  ╷  c2
  ╷
  ╷ o  49cdb4091  Today at 00:00  test  feature1
  ╭─╯  b
  │
  o  b68836a6e  Today at 00:00  test
  │  a2
  ~

  $ USER=test SL_CONFIG_PATH="$SL_CONFIG_PATH;fb=static" sl smartlog -v -T '{sl}' --commit-info --config extensions.commitcloud=!
    @  05d102502  Today at 00:00  test  feature2*
  ╭─╯  d
  │
  │     M c2
  │     A d
  │
  o  38d85b506  Today at 00:00  test  master public/38d85b50675416788125247211034703776317bd
  ╷  c2
  ╷
  ╷ o  49cdb4091  Today at 00:00  test  feature1
  ╭─╯  b
  │
  │     A b
  │
  o  b68836a6e  Today at 00:00  test
  │  a2
  ~

As a revset
  $ sl log -G -T '{node|short} {bookmarks} {desc}' -r 'smartlog()'
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

  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --master 'desc(a2)'
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
  $ sl smartlog -T '{node|short} {bookmarks} {desc}' -r 'desc(b)' -r 'desc(c2)' --master null
  o  49cdb4091aca feature1 b
  │
  │ o  38d85b506754 master c2
  ├─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ sl smartlog -T '{node|short} {bookmarks} {desc}' -r 'smartlog()' -r 'desc(a1)'
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
  $ sl debugmakepublic 49cdb4091aca

  $ sl bookmarks -f master -r 49cdb4091aca
  $ sl smartlog -T '{node|short} {bookmarks} {desc}'
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
  $ sl debugmakepublic 38d85b506754

  $ sl bookmarks -f master -r 38d85b506754
  $ sl smartlog -T '{node|short} {bookmarks} {desc}'
  @  05d10250273e feature2 d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ sl debugmakepublic feature1

  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --master feature1
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

  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --config smartlog.master=feature1
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

  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --config smartlog.master=feature2 --master feature1
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

  $ sl debugmakepublic .

Test with weird bookmark names

  $ sl book -r 'desc(b)' foo-bar
  $ sl smartlog -r 'foo-bar + .' -T '{node|short} {bookmarks} {desc}'
  @  05d10250273e feature2 d
  │
  o  38d85b506754 master c2
  ╷
  ╷ o  49cdb4091aca feature1 foo-bar b
  ╭─╯
  o  b68836a6e2ca  a2
  │
  ~

  $ sl debugmakepublic foo-bar

  $ sl smartlog --config smartlog.master=foo-bar -T '{node|short} {bookmarks} {desc}'
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
  $ sl smartlog --config smartlog.master=xxxx -T '{node|short} {bookmarks} {desc}'
  abort: unknown revision 'xxxx'!
  [255]

Test with two unrelated histories
  $ sl goto null
  0 files updated, 0 files merged, 5 files removed, 0 files unresolved
  (leaving bookmark feature2)
  $ touch u1 && sl add u1 && sl ci -mu1
  $ touch u2 && sl add u2 && sl ci -mu2

  $ sl smartlog  -T '{node|short} {bookmarks} {desc}'
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
  $ sl init repo2
  $ cd repo2
  $ sl debugbuilddag '+4'
  $ sl bookmark curr
  $ sl bookmark master -r 'desc(r1)'
  $ sl debugmakepublic -r 'desc(r1)'
  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --all
  o  2dc09a01254d  r3
  │
  o  01241442b3c2  r2
  │
  o  66f7d451a68b master r1
  │
  ~
  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
    o  2dc09a01254d  r3
    │
    o  01241442b3c2  r2
  ╭─╯
  o  66f7d451a68b master r1
  │
  ~

Different number of lines per node

  $ sl smartlog -T '{node|short} {bookmarks} {desc} {author} {date|isodate}' --all --config smartlog.indentnonpublic=1
    o  2dc09a01254d  r3 debugbuilddag 1970-01-01 00:00 +0000
    │
    o  01241442b3c2  r2 debugbuilddag 1970-01-01 00:00 +0000
  ╭─╯
  o  66f7d451a68b master r1 debugbuilddag 1970-01-01 00:00 +0000
  │
  ~

Add other draft stacks
  $ sl up 'desc(r1)' -q
  $ echo 1 > a
  $ sl ci -A a -m a -q
  $ echo 2 >> a
  $ sl ci -A a -m a -q
  $ sl up 'desc(r2)' -q
  $ echo 2 > b
  $ sl ci -A b -m b -q
  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
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

  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.max-commit-threshold=2
  smartlog: too many (6) commits, not rendering all of them
  (consider running 'sl doctor' to hide unrelated commits)
  @  401cd6213b51  b
  ╷
  ╷ o  a60fccdcd9e9  a
  ╭─╯
  ╷
  ╷ o  2dc09a01254d  r3
  ╭─╯
  ╷
  o  66f7d451a68b master r1
  │
  ~

Recent arg select days correctly
  $ echo 1 >> b
  $ setconfig devel.default-date='2020-5-30'
  $ sl commit --date "20 days ago" -m test2
  $ sl goto 'desc(r0)' -q
  $ sl log -Gr 'smartlog(master="master", heads=((date(-15) & draft()) + .))' -T '{node|short} {bookmarks} {desc}'
  o  66f7d451a68b master r1
  │
  @  1ea73414a91b  r0
  

  $ sl log -Gr 'smartlog((date(-25) & draft()) + .)' -T '{bookmarks} {desc}'
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
  $ sl init repo3
  $ cd repo3
  $ sl debugbuilddag '+5'
  $ sl bookmark master -r 'desc(r1)'
  $ sl debugmakepublic -r 'desc(r1)'
  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
    o  bebd167eb94d  r4
    │
    o  2dc09a01254d  r3
    │
    o  01241442b3c2  r2
  ╭─╯
  o  66f7d451a68b master r1
  │
  ~
  $ sl debugmakepublic 'desc(r3)'
  $ sl up -q 'desc(r4)'
  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
    @  bebd167eb94d  r4
  ╭─╯
  o  2dc09a01254d  r3
  ╷
  o  66f7d451a68b master r1
  │
  ~
  $ sl debugmakepublic 'desc(r4)'
  $ sl smartlog -T '{node|short} {bookmarks} {desc}' --all --config smartlog.indentnonpublic=1
  @  bebd167eb94d  r4
  ╷
  o  66f7d451a68b master r1
  │
  ~

Indent non-public when public rev is 0:

  $ newrepo
  $ echo 'A-B' | drawdag
  $ sl debugmakepublic $A
  $ sl dbsh -c "cl.inner.flush([bin('$A')])"  # reassign $A to "master group" so its rev is 0
  $ sl smartlog -T '{rev} {desc} {phase}' --config smartlog.indentnonpublic=1
    o  281474976710656 B draft
  ╭─╯
  o  0 A public

