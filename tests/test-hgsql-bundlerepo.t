  $ . "$TESTDIR/hgsql/library.sh"

Populate the db with an initial commit

  $ initclient client
  $ cd client
  $ echo x > x
  $ hg commit -qAm x
  $ cd ..

  $ initserver master masterrepo
  $ initserver master2 masterrepo
  $ cd master
  $ hg log
  $ hg pull -q ../client

  $ cd ..

Test viewing a bundle repo
  $ cd client
  $ echo y > x
  $ hg commit -qAm x2
  $ hg bundle --base 0 --rev 1 ../mybundle.hg
  1 changesets found

  $ cd ../master
  $ hg -R ../mybundle.hg log -r tip -T '{rev} {desc}\n'
  1 x2
  $ hg log -r tip -T '{rev} {desc}\n'
  0 x
