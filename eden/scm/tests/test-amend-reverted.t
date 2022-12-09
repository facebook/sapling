#chg-compatible
#debugruntest-compatible

  $ enable amend
  $ setconfig diff.git=1

  $ configure mutation-norecord

Basic amend

  $ hg init repo1
  $ cd repo1
  $ hg debugdrawdag <<'EOS'
  > B
  > |
  > A
  > EOS

  $ hg goto B -q
  $ echo 2 >> B
  $ hg amend
  $ hg log -r . -T '{files}'
  B (no-eol)
  $ hg st

Now revert and amend file B, we should get an empty commit
  $ hg revert -r .^ B
  $ hg amend
  $ hg st
  $ hg log -r . -T '{files}'


Create a commit with a few files, revert a few of them
and then amend them one by one
  $ echo 1 > 1
  $ echo 2 > 2
  $ echo 3 > 3
  $ hg add 1 2 3
  $ hg ci -m '1 2 3'
  $ hg revert -r .^ 1
  $ hg revert -r .^ 2

Now amend a single file
  $ hg st
  R 1
  R 2
  $ hg amend 1
  $ hg st
  R 2
  $ hg log -r . -T '{files}'
  2 3 (no-eol)

Now amend the second file
  $ hg amend 2
  $ hg st
  $ hg log -r . -T '{files}'
  3 (no-eol)

Now rename a file and amend
  $ hg mv 3 33
  $ hg amend
  $ hg st
  $ hg log -r . -T '{files}'
  33 (no-eol)

  $ hg mv 33 333
  $ hg amend 333
  $ hg log -r . -T '{files}'
  33 333 (no-eol)


Create a commit with two files, then change these files in another
commit, then revert two of them and then amend a single one
  $ echo x > x
  $ echo y > y
  $ hg add x y
  $ hg ci -m 'x y'
  $ echo xx > x
  $ echo yy > y
  $ hg ci -m 'xx yy'
  $ hg revert -r .^ x
  $ hg revert -r .^ y
  $ hg amend x
  $ hg st
  M y
  $ hg log -r . -T '{files}'
  y (no-eol)

