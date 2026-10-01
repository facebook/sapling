#modern-config-incompatible
#require no-eden

The bundle push path (ssh server) refuses conflict-free merges as well.

  $ configure mutation-norecord dummyssh
  $ enable amend
  $ sl init server
  $ sl clone -q ssh://user@dummy/server repo
  $ cd repo
  $ echo a > a
  $ sl commit -Aqm a
  $ sl push -q -r . --to master --create
  $ sl debugmakepublic .
  $ echo b > b
  $ sl commit -Aqm b
  $ sl goto -q '.^'
  $ echo c > c
  $ sl commit -Aqm c
  $ sl merge -q --noconflict -m m 'desc(b)'
  $ sl push -r . --to master
  pushing rev * to destination ssh://user@dummy/server bookmark master (glob)
  searching for changes
  abort: cannot push conflict-free merge:
    * m (glob)
  (such a merge only records that its parents can be merged automatically; push or land the parents and descendants instead)
  [255]

Every conflict-free merge in the push is named
  $ echo d > d
  $ sl commit -Aqm d
  $ sl goto -q '.^'
  $ echo e > e
  $ sl commit -Aqm e
  $ sl merge -q --noconflict -m m2 'desc(d)'
  $ sl push -r . --to master
  pushing rev * to destination ssh://user@dummy/server bookmark master (glob)
  searching for changes
  abort: cannot push conflict-free merges:
    * m2 (glob)
    * m (glob)
  (such a merge only records that its parents can be merged automatically; push or land the parents and descendants instead)
  [255]

The check can be switched off
  $ sl push -q -r . --to master --config push.reject-noconflict-merges=false
  $ sl log -r 'remote/master' -T '{desc}\n'
  m2
