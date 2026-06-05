#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ sl init repo
  $ cd repo
  $ cat >> .sl/config <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ sl commit -qAm x
  $ sl book master
  $ echo x >> x
  $ sl commit -qAm x2

Test that query parameters are ignored when grouping paths, so that
when pushing to one path, the bookmark for the other path gets updated
as well

  $ cd ..
  $ hgcloneshallow ssh://user@dummy/repo client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)
  $ cd client
  $ sl path
  default = ssh://user@dummy/repo
  $ sl path -a default ssh://user@dummy/repo?read
  $ sl path -a default-push ssh://user@dummy/repo?write
  $ sl path
  default = ssh://user@dummy/repo?read
  default-push = ssh://user@dummy/repo?write
  $ sl log -r .
  commit:      a89d614e2364
  bookmark:    remote/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x2
  
  $ echo x >> x
  $ sl commit -qAm x3
  $ sl push --to master
  pushing rev 421535db10b6 to destination ssh://user@dummy/repo?write bookmark master
  searching for changes
  updating bookmark master
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  $ sl log -r .
  commit:      421535db10b6
  bookmark:    remote/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x3
  
  $ sl pull
  pulling from ssh://user@dummy/repo?read
  $ sl log -r .
  commit:      421535db10b6
  bookmark:    remote/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x3
  
