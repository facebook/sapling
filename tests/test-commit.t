commit date test

  $ hg init test
  $ cd test
  $ echo foo > foo
  $ hg add foo
  $ HGEDITOR=true hg commit -m ""
  abort: empty commit message
  [255]
  $ hg commit -d '0 0' -m commit-1
  $ echo foo >> foo
  $ hg commit -d '1 4444444' -m commit-3
  abort: impossible time zone offset: 4444444
  [255]
  $ hg commit -d '1	15.1' -m commit-4
  abort: invalid date: '1\t15.1'
  [255]
  $ hg commit -d 'foo bar' -m commit-5
  abort: invalid date: 'foo bar'
  [255]
  $ hg commit -d ' 1 4444' -m commit-6
  $ hg commit -d '111111111111 0' -m commit-7
  abort: date exceeds 32 bits: 111111111111
  [255]
  $ hg commit -d '-7654321 3600' -m commit-7
  abort: negative date value: -7654321
  [255]

commit added file that has been deleted

  $ echo bar > bar
  $ hg add bar
  $ rm bar
  $ hg commit -m commit-8
  nothing changed (1 missing files, see 'hg status')
  [1]
  $ hg commit -m commit-8-2 bar
  abort: bar: file not found!
  [255]

  $ hg -q revert -a --no-backup

  $ mkdir dir
  $ echo boo > dir/file
  $ hg add
  adding dir/file
  $ hg -v commit -m commit-9 dir
  dir/file
  committed changeset 2:d2a76177cb42

  $ echo > dir.file
  $ hg add
  adding dir.file
  $ hg commit -m commit-10 dir dir.file
  abort: dir: no match under directory!
  [255]

  $ echo >> dir/file
  $ mkdir bleh
  $ mkdir dir2
  $ cd bleh
  $ hg commit -m commit-11 .
  abort: bleh: no match under directory!
  [255]
  $ hg commit -m commit-12 ../dir ../dir2
  abort: dir2: no match under directory!
  [255]
  $ hg -v commit -m commit-13 ../dir
  dir/file
  committed changeset 3:1cd62a2d8db5
  $ cd ..

  $ hg commit -m commit-14 does-not-exist
  abort: does-not-exist: No such file or directory
  [255]
  $ ln -s foo baz
  $ hg commit -m commit-15 baz
  abort: baz: file not tracked!
  [255]
  $ touch quux
  $ hg commit -m commit-16 quux
  abort: quux: file not tracked!
  [255]
  $ echo >> dir/file
  $ hg -v commit -m commit-17 dir/file
  dir/file
  committed changeset 4:49176991390e

An empty date was interpreted as epoch origin

  $ echo foo >> foo
  $ hg commit -d '' -m commit-no-date
  $ hg tip --template '{date|isodate}\n' | grep '1970'
  [1]

Make sure we do not obscure unknown requires file entries (issue2649)

  $ echo foo >> foo
  $ echo fake >> .hg/requires
  $ hg commit -m bla
  abort: unknown repository format: requires features 'fake' (upgrade Mercurial)!
  [255]

  $ cd ..


partial subdir commit test

  $ hg init test2
  $ cd test2
  $ mkdir foo
  $ echo foo > foo/foo
  $ mkdir bar
  $ echo bar > bar/bar
  $ hg add
  adding bar/bar
  adding foo/foo
  $ hg ci -m commit-subdir-1 foo
  $ hg ci -m commit-subdir-2 bar

subdir log 1

  $ hg log -v foo
  changeset:   0:f97e73a25882
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/foo
  description:
  commit-subdir-1
  
  

subdir log 2

  $ hg log -v bar
  changeset:   1:aa809156d50d
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/bar
  description:
  commit-subdir-2
  
  

full log

  $ hg log -v
  changeset:   1:aa809156d50d
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       bar/bar
  description:
  commit-subdir-2
  
  
  changeset:   0:f97e73a25882
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/foo
  description:
  commit-subdir-1
  
  
  $ cd ..


dot and subdir commit test

  $ hg init test3
  $ cd test3
  $ mkdir foo
  $ echo foo content > foo/plain-file
  $ hg add foo/plain-file
  $ hg ci -m commit-foo-subdir foo
  $ echo modified foo content > foo/plain-file
  $ hg ci -m commit-foo-dot .

full log

  $ hg log -v
  changeset:   1:95b38e3a5b2e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/plain-file
  description:
  commit-foo-dot
  
  
  changeset:   0:65d4e9386227
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo/plain-file
  description:
  commit-foo-subdir
  
  

subdir log

  $ cd foo
  $ hg log .
  changeset:   1:95b38e3a5b2e
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-foo-dot
  
  changeset:   0:65d4e9386227
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit-foo-subdir
  
  $ cd ..
  $ cd ..

Issue1049: Hg permits partial commit of merge without warning

  $ cd ..
  $ hg init issue1049
  $ cd issue1049
  $ echo a > a
  $ hg ci -Ama
  adding a
  $ echo a >> a
  $ hg ci -mb
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> a
  $ hg ci -mc
  created new head
  $ HGMERGE=true hg merge
  merging a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

should fail because we are specifying a file name

  $ hg ci -mmerge a
  abort: cannot partially commit a merge (do not specify files or patterns)
  [255]

should fail because we are specifying a pattern

  $ hg ci -mmerge -I a
  abort: cannot partially commit a merge (do not specify files or patterns)
  [255]

should succeed

  $ hg ci -mmerge
  $ cd ..


test commit message content

  $ hg init commitmsg
  $ cd commitmsg
  $ echo changed > changed
  $ echo removed > removed
  $ hg ci -qAm init

  $ hg rm removed
  $ echo changed >> changed
  $ echo added > added
  $ hg add added
  $ HGEDITOR=cat hg ci -A
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: added added
  HG: changed changed
  HG: removed removed
  abort: empty commit message
  [255]
