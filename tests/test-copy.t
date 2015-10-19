# enable bundle2 in advance

  $ cat << EOF >> $HGRCPATH
  > [format]
  > usegeneraldelta=yes
  > EOF

  $ mkdir part1
  $ cd part1

  $ hg init
  $ echo a > a
  $ hg add a
  $ hg commit -m "1"
  $ hg status
  $ hg copy a b
  $ hg --config ui.portablefilenames=abort copy a con.xml
  abort: filename contains 'con', which is reserved on Windows: 'con.xml'
  [255]
  $ hg status
  A b
  $ hg sum
  parent: 0:c19d34741b0a tip
   1
  branch: default
  commit: 1 copied
  update: (current)
  phases: 1 draft
  $ hg --debug commit -m "2"
  committing files:
  b
   b: copy a:b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3
  committing manifest
  committing changelog
  committed changeset 1:93580a2c28a50a56f63526fb305067e6fbf739c4

we should see two history entries

  $ hg history -v
  changeset:   1:93580a2c28a5
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       b
  description:
  2
  
  
  changeset:   0:c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  1
  
  

we should see one log entry for a

  $ hg log a
  changeset:   0:c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  

this should show a revision linked to changeset 0

  $ hg debugindex a
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       3  .....       0 b789fdd96dc2 000000000000 000000000000 (re)

we should see one log entry for b

  $ hg log b
  changeset:   1:93580a2c28a5
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  

this should show a revision linked to changeset 1

  $ hg debugindex b
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0      65  .....       1 37d9b5d994ea 000000000000 000000000000 (re)

this should show the rename information in the metadata

  $ hg debugdata b 0 | head -3 | tail -2
  copy: a
  copyrev: b789fdd96dc2f3bd229c1dd8eedf0fc60e2b68e3

  $ md5sum.py .hg/store/data/b.i
  44913824c8f5890ae218f9829535922e  .hg/store/data/b.i
  $ hg cat b > bsum
  $ md5sum.py bsum
  60b725f10c9c85c70d97880dfe8191b3  bsum
  $ hg cat a > asum
  $ md5sum.py asum
  60b725f10c9c85c70d97880dfe8191b3  asum
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 2 changesets, 2 total revisions

  $ cd ..


  $ mkdir part2
  $ cd part2

  $ hg init
  $ echo foo > foo
should fail - foo is not managed
  $ hg mv foo bar
  foo: not copying - file is not managed
  abort: no files to copy
  [255]
  $ hg st -A
  ? foo
  $ hg add foo
dry-run; print a warning that this is not a real copy; foo is added
  $ hg mv --dry-run foo bar
  foo has not been committed yet, so no copy data will be stored for bar.
  $ hg st -A
  A foo
should print a warning that this is not a real copy; bar is added
  $ hg mv foo bar
  foo has not been committed yet, so no copy data will be stored for bar.
  $ hg st -A
  A bar
should print a warning that this is not a real copy; foo is added
  $ hg cp bar foo
  bar has not been committed yet, so no copy data will be stored for foo.
  $ hg rm -f bar
  $ rm bar
  $ hg st -A
  A foo
  $ hg commit -m1

moving a missing file
  $ rm foo
  $ hg mv foo foo3
  foo: deleted in working directory
  foo3 does not exist!
  $ hg up -qC .

copy --after to a nonexistent target filename
  $ hg cp -A foo dummy
  foo: not recording copy - dummy does not exist

dry-run; should show that foo is clean
  $ hg copy --dry-run foo bar
  $ hg st -A
  C foo
should show copy
  $ hg copy foo bar
  $ hg st -C
  A bar
    foo

shouldn't show copy
  $ hg commit -m2
  $ hg st -C

should match
  $ hg debugindex foo
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       5  .....       0 2ed2a3912a0b 000000000000 000000000000 (re)
  $ hg debugrename bar
  bar renamed from foo:2ed2a3912a0b24502043eae84ee4b279c18b90dd

  $ echo bleah > foo
  $ echo quux > bar
  $ hg commit -m3

should not be renamed
  $ hg debugrename bar
  bar not renamed

  $ hg copy -f foo bar
should show copy
  $ hg st -C
  M bar
    foo

XXX: filtering lfilesrepo.status() in 3.3-rc causes the copy source to not be
displayed.
  $ hg st -C --config extensions.largefiles=
  M bar
    foo

  $ hg commit -m3

should show no parents for tip
  $ hg debugindex bar
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0      69  .....       1 7711d36246cc 000000000000 000000000000 (re)
       1        69       6  .....       2 bdf70a2b8d03 7711d36246cc 000000000000 (re)
       2        75      71  .....       3 b2558327ea8d 000000000000 000000000000 (re)
should match
  $ hg debugindex foo
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       5  .....       0 2ed2a3912a0b 000000000000 000000000000 (re)
       1         5       7  .....       2 dd12c926cf16 2ed2a3912a0b 000000000000 (re)
  $ hg debugrename bar
  bar renamed from foo:dd12c926cf165e3eb4cf87b084955cb617221c17

should show no copies
  $ hg st -C

copy --after on an added file
  $ cp bar baz
  $ hg add baz
  $ hg cp -A bar baz
  $ hg st -C
  A baz
    bar

foo was clean:
  $ hg st -AC foo
  C foo
but it's considered modified after a copy --after --force
  $ hg copy -Af bar foo
  $ hg st -AC foo
  M foo
    bar

  $ cd ..
