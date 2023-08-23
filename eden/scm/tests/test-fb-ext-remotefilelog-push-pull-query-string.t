#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > remotenames=
  > EOF

  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ hg book master
  $ echo x >> x
  $ hg commit -qAm x2

Test that query parameters are ignored when grouping paths, so that
when pushing to one path, the bookmark for the other path gets updated
as well

  $ cd ..
  $ hgcloneshallow ssh://user@dummy/repo client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)
  $ cd client
  $ hg path
  default = ssh://user@dummy/repo
  $ hg path -a default ssh://user@dummy/repo?read
  $ hg path -a default-push ssh://user@dummy/repo?write
  $ hg path
  default = ssh://user@dummy/repo?read
  default-push = ssh://user@dummy/repo?write
  $ hg log -r .
  commit:      a89d614e2364
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x2
  
  $ echo x >> x
  $ hg commit -qAm x3
  $ hg push --to master
  pushing rev 421535db10b6 to destination ssh://user@dummy/repo?write bookmark master
  searching for changes
  updating bookmark master
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  $ hg log -r .
  commit:      421535db10b6
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x3
  
  $ hg pull
  pulling from ssh://user@dummy/repo?read
  searching for changes
  no changes found
  $ hg log -r .
  commit:      421535db10b6
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x3
  
