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

copy --after to a nonexistant target filename
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
  $ hg debugindex .hg/store/data/foo.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       5      0       0 2ed2a3912a0b 000000000000 000000000000
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
  $ hg commit -m3

should show no parents for tip
  $ hg debugindex .hg/store/data/bar.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      69      0       1 7711d36246cc 000000000000 000000000000
       1        69       6      1       2 bdf70a2b8d03 7711d36246cc 000000000000
       2        75      81      1       3 b2558327ea8d 000000000000 000000000000
should match
  $ hg debugindex .hg/store/data/foo.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       5      0       0 2ed2a3912a0b 000000000000 000000000000
       1         5       7      1       2 dd12c926cf16 2ed2a3912a0b 000000000000
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
  $  hg st -AC foo
  C foo
but it's considered modified after a copy --after --force
  $ hg copy -Af bar foo
  $ hg st -AC foo
  M foo
    bar
