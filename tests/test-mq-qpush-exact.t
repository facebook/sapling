  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "graphlog=" >> $HGRCPATH

make a test repository that looks like this:

o    2:28bc7b1afd6a
|
| @  1:d7fe2034f71b
|/
o    0/62ecad8b70e5

  $ hg init r0
  $ cd r0
  $ touch f0
  $ hg ci -m0 -Aq
  $ touch f1
  $ hg ci -m1 -Aq

  $ hg update 0 -q
  $ touch f2
  $ hg ci -m2 -Aq
  $ hg update 1 -q

make some patches with a parent: 1:d7fe2034f71b -> p0 -> p1

  $ echo cp0 >> fp0
  $ hg add fp0
  $ hg ci -m p0 -d "0 0"
  $ hg export -r. > p0
  $ hg strip -qn .
  $ hg qimport p0
  adding p0 to series file
  $ hg qpush
  applying p0
  now at: p0

  $ echo cp1 >> fp1
  $ hg add fp1
  $ hg qnew p1 -d "0 0"

  $ hg qpop -aq
  patch queue now empty

qpush --exact when at the parent

  $ hg update 1 -q
  $ hg qpush -e
  applying p0
  now at: p0
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg qpush -e p0
  applying p0
  now at: p0
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg qpush -e p1
  applying p0
  applying p1
  now at: p1
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

qpush --exact when at another rev

  $ hg update 0 -q
  $ hg qpush -e
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  applying p0
  now at: p0
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg update 0 -q
  $ hg qpush -e p0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  applying p0
  now at: p0
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg update 0 -q
  $ hg qpush -e p1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  applying p0
  applying p1
  now at: p1
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg update 0 -q
  $ hg qpush -ea
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  applying p0
  applying p1
  now at: p1
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

qpush --exact while crossing branches

  $ hg update 2 -q
  $ hg qpush -e
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  applying p0
  now at: p0
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg update 2 -q
  $ hg qpush -e p0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  applying p0
  now at: p0
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg update 2 -q
  $ hg qpush -e p1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  applying p0
  applying p1
  now at: p1
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

  $ hg update 2 -q
  $ hg qpush -ea
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  applying p0
  applying p1
  now at: p1
  $ hg parents -qr qbase
  1:d7fe2034f71b
  $ hg qpop -aq
  patch queue now empty

qpush --exact --force with changes to an unpatched file

  $ hg update 1 -q
  $ echo c0 >> f0
  $ hg qpush -e
  abort: local changes found
  [255]
  $ hg qpush -ef
  applying p0
  now at: p0
  $ cat f0
  c0
  $ rm f0
  $ touch f0
  $ hg qpop -aq
  patch queue now empty

  $ hg update 1 -q
  $ echo c0 >> f0
  $ hg qpush -e p1
  abort: local changes found
  [255]
  $ hg qpush -e p1 -f
  applying p0
  applying p1
  now at: p1
  $ cat f0
  c0
  $ rm f0
  $ touch f0
  $ hg qpop -aq
  patch queue now empty

qpush --exact --force with changes to a patched file

  $ hg update 1 -q
  $ echo cp0-bad >> fp0
  $ hg add fp0
  $ hg qpush -e
  abort: local changes found
  [255]
  $ hg qpush -ef
  applying p0
  file fp0 already exists
  1 out of 1 hunks FAILED -- saving rejects to file fp0.rej
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  errors during apply, please fix and refresh p0
  [2]
  $ cat fp0
  cp0-bad
  $ cat fp0.rej
  --- fp0
  +++ fp0
  @@ -0,0 +1,1 @@
  +cp0
  $ hg qpop -aqf
  patch queue now empty
  $ rm fp0
  $ rm fp0.rej

  $ hg update 1 -q
  $ echo cp1-bad >> fp1
  $ hg add fp1
  $ hg qpush -e p1
  abort: local changes found
  [255]
  $ hg qpush -e p1 -f
  applying p0
  applying p1
  file fp1 already exists
  1 out of 1 hunks FAILED -- saving rejects to file fp1.rej
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  errors during apply, please fix and refresh p1
  [2]
  $ cat fp1
  cp1-bad
  $ cat fp1.rej
  --- fp1
  +++ fp1
  @@ -0,0 +1,1 @@
  +cp1
  $ hg qpop -aqf
  patch queue now empty
  $ hg forget fp1
  $ rm fp1
  $ rm fp1.rej

qpush --exact when already at a patch

  $ hg update 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg qpush -e p0
  applying p0
  now at: p0
  $ hg qpush -e p1
  abort: cannot push --exact with applied patches
  [255]
  $ hg qpop -aq
  patch queue now empty

qpush --exact --move should fail

  $ hg qpush -e --move p1
  abort: cannot use --exact and --move together
  [255]

qpush --exact a patch without a parent recorded

  $ hg qpush -q
  now at: p0
  $ grep -v '# Parent' .hg/patches/p0 > p0.new
  $ mv p0.new .hg/patches/p0
  $ hg qpop -aq
  patch queue now empty
  $ hg qpush -e
  abort: p0 does not have a parent recorded
  [255]
  $ hg qpush -e p0
  abort: p0 does not have a parent recorded
  [255]
  $ hg qpush -e p1
  abort: p0 does not have a parent recorded
  [255]
  $ hg qpush -ea
  abort: p0 does not have a parent recorded
  [255]

  $ cd ..
