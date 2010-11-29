  $ hg init
  $ hg init sub
  $ echo 'sub = sub' > .hgsub
  $ hg add .hgsub
  $ echo c1 > f1
  $ echo c2 > sub/f2
  $ hg add -S
  adding f1
  adding sub/f2
  $ hg commit -m0
  committing subrepository sub

Make .hgsubstate dirty:

  $ echo '0000000000000000000000000000000000000000 sub' > .hgsubstate
  $ hg diff --nodates
  diff -r 853ea21970bb .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -5bbc614a5b06ad7f3bf7c2463d74b005324f34c1 sub
  +0000000000000000000000000000000000000000 sub

trying to do an empty commit:

  $ hg commit -m1
  committing subrepository sub
  nothing changed
  [1]

an okay update of .hgsubstate
  $ cd sub
  $ echo c3 > f2
  $ hg commit -m "Sub commit"
  $ cd ..
  $ hg commit -m "Updated sub"
  committing subrepository sub

deleting again:
  $ echo '' > .hgsub
  $ hg commit -m2
  $ cat .hgsub
  
  $ cat .hgsubstate

an okay commit, but with a dirty .hgsubstate
  $ echo 'sub = sub' > .hgsub
  $ hg commit -m3
  committing subrepository sub
  $ echo '0000000000000000000000000000000000000000 sub' > .hgsubstate
  $ hg diff --nodates
  diff -r 41e1dee3d5d9 .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -fe0229ee9a0a38b43163c756bb51b94228b118e7 sub
  +0000000000000000000000000000000000000000 sub
  $ echo c4 > f3
  $ hg add f3
  $ hg status 
  M .hgsubstate
  A f3
  $ hg commit -m4
  committing subrepository sub
