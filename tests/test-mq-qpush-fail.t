Test that qpush cleans things up if it doesn't complete

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ hg init repo
  $ cd repo
  $ echo foo > foo
  $ hg ci -Am 'add foo'
  adding foo
  $ touch untracked-file
  $ echo 'syntax: glob' > .hgignore
  $ echo '.hgignore' >> .hgignore
  $ hg qinit

test qpush on empty series

  $ hg qpush
  no patches in series
  $ hg qnew patch1
  $ echo >> foo
  $ hg qrefresh -m 'patch 1'
  $ hg qnew patch2
  $ echo bar > bar
  $ hg add bar
  $ hg qrefresh -m 'patch 2'
  $ hg qnew --config 'mq.plain=true' bad-patch
  $ echo >> foo
  $ hg qrefresh
  $ hg qpop -a
  popping bad-patch
  popping patch2
  popping patch1
  patch queue now empty
  $ python -c 'print "\xe9"' > message
  $ cat .hg/patches/bad-patch >> message
  $ mv message .hg/patches/bad-patch
  $ hg qpush -a && echo 'qpush succeded?!'
  applying patch1
  applying patch2
  applying bad-patch
  transaction abort!
  rollback completed
  cleaning up working directory...done
  abort: decoding near 'é': 'ascii' codec can't decode byte 0xe9 in position 0: ordinal not in range(128)!
  [255]
  $ hg parents
  changeset:   0:bbd179dfa0a7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add foo
  

bar should be gone; other unknown/ignored files should still be around

  $ hg status -A
  ? untracked-file
  I .hgignore
  C foo

preparing qpush of a missing patch

  $ hg qpop -a
  no patches applied
  $ hg qpush
  applying patch1
  now at: patch1
  $ rm .hg/patches/patch2

now we expect the push to fail, but it should NOT complain about patch1

  $ hg qpush
  applying patch2
  unable to read patch2
  now at: patch1
  [1]

preparing qpush of missing patch with no patch applied

  $ hg qpop -a
  popping patch1
  patch queue now empty
  $ rm .hg/patches/patch1

qpush should fail the same way as below

  $ hg qpush
  applying patch1
  unable to read patch1
  [1]
