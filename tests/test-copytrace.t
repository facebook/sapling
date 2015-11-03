  $ extpath=$(dirname $TESTDIR)
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > copytrace=$extpath/copytrace
  > rebase=
  > EOF



FIRST TEST:

Cases tested:
deleted     renamed
renamed     modified
modified    renamed

*=modified

.    bbb c**
.      :
.   aa bb c**
.      :
.   a* b* c*  aaa b* cc
.      :        :
.    a b c  ....

Setup repo

  $ hg init repo
  $ cd repo
  $ echo 'foo' > a
  $ echo 'bar' > b
  $ echo 'foobar' > c
  $ hg add a b c
  $ hg commit -m "added a b c"

  $ hg mv a aaa
  $ hg mv c cc
  $ echo 'bar' >> b
  $ hg commit -m "mv a c, mod b"
  $ hg up .^ -q

  $ echo 'foo' >> a
  $ echo 'bar' >> b
  $ echo 'foobar' >> c
  $ hg commit -m "mod a b c" -q

  $ hg mv a aa
  $ hg mv b bb
  $ echo 'foobar' >> c
  $ hg commit -m "mv a b, mod c"

  $ hg rm aa
  $ hg mv bb bbb
  $ hg commit -m "del a, mv b"

  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 8762e63a42ae10308388b8a3f1cc820b6bd31e05
  |   desc: del a, mv b
  o  changeset: df8ac2088ab6a0c9ba90a4b8457fb83b6eb25661
  |   desc: mv a b, mod c
  o  changeset: d7d8227aa33d850c595b730fb42f31bb5c299e26
  |   desc: mod a b c
  | o  changeset: 90e435664a9d37eeb4bab59e08166a3e788fd602
  |/    desc: mv a c, mod b
  o  changeset: 2dcedc870147eb1b234c36216e40e4c52fc8b157
      desc: added a b c

Rebase
  $ hg rebase -s 90e435664a -d 8762e63a42
  rebasing 1:90e435664a9d "mv a c, mod b"
  merging c and cc to cc
  note: possible conflict - a was deleted and renamed to:
   aaa
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/90e435664a9d-d4904a7d-backup.hg (glob)

  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  o  changeset: 1bc29be2c9e668e5a0d65a6ab7aed400c5e27841
  |   desc: mv a c, mod b
  @  changeset: 8762e63a42ae10308388b8a3f1cc820b6bd31e05
  |   desc: del a, mv b
  o  changeset: df8ac2088ab6a0c9ba90a4b8457fb83b6eb25661
  |   desc: mv a b, mod c
  o  changeset: d7d8227aa33d850c595b730fb42f31bb5c299e26
  |   desc: mod a b c
  o  changeset: 2dcedc870147eb1b234c36216e40e4c52fc8b157
      desc: added a b c
  $ hg up -q 1bc29be2c9e66
  $ ls
  aaa
  bbb
  cc
  $ cat aaa
  foo
  $ cat bbb
  bar
  bar
  $ cat cc
  foobar
  foobar
  foobar

  $ cd ..
  $ rm -rf repo



SECOND TEST:

Cases tested:
renamed     deleted
renamed     renamed


.    aa bb     bbb
.      :        :
.     a b   ....

Setup repo

  $ hg init repo
  $ cd repo
  $ echo 'foo' > a
  $ echo 'bar' > b
  $ hg add a b
  $ hg commit -m "added a b"

  $ hg rm a
  $ hg mv b bbb
  $ hg commit -m "del a, mv b"
  $ hg update -q .^

  $ hg mv a aa
  $ hg mv b bb
  $ hg commit -m "mv a b" -q

  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: a7b3b06dfc8cccf6be4646d03d458221368bca60
  |   desc: mv a b
  | o  changeset: 42c403609d69c78901851becf4c4b85543eaadfa
  |/    desc: del a, mv b
  o  changeset: cc218bc7593246156e761e5477a5db40e26aabde
      desc: added a b

Rebase

  $ hg rebase -s 42c403609d -d a7b3b06df
  rebasing 1:42c403609d69 "del a, mv b"
  note: possible conflict - b was renamed multiple times to:
   bb
   bbb
  note: possible conflict - a was deleted and renamed to:
   aa
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/42c403609d69-47b673fa-backup.hg (glob)
  $ hg log -G -T 'changeset: {node}\n desc: {desc}'
  o  changeset: e54630eebe60d7a0cd6f46913efc309301d303ea
  |   desc: del a, mv b
  @  changeset: a7b3b06dfc8cccf6be4646d03d458221368bca60
  |   desc: mv a b
  o  changeset: cc218bc7593246156e761e5477a5db40e26aabde
      desc: added a b

  $ hg update -q e54630eebe60d7
  $ ls
  aa
  bb
  bbb

  $ cd ..
  $ rm -rf repo



THIRD TEST

Cases tested:
Branch rebase

.              a e
.               :
.     c b      a d
.      :        :
.     a b  ....


Setup repo

  $ hg init repo
  $ cd repo
  $ echo 'foo' > a
  $ echo 'bar' > b
  $ hg add a b
  $ hg commit -m "added a b"
  $ hg mv a c
  $ echo 'foo' >> b
  $ hg commit -m "mv a c, mod b"
  $ hg update -q .^
  $ hg mv b d
  $ hg commit -q -m "mv b d"
  $ hg mv d e
  $ echo 'bar' >> a
  $ hg commit -m "mv d e, mod a"
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 44e133f71df318bc964e5240576baa851f1ba505
  |   desc: mv d e, mod a
  o  changeset: ed80d1d38c22b832268f774f8a71f8d4dd6625e2
  |   desc: mv b d
  | o  changeset: 2278324ad33018f045aa853237766854f431307a
  |/    desc: mv a c, mod b
  o  changeset: cc218bc7593246156e761e5477a5db40e26aabde
      desc: added a b

Rebase

  $ hg rebase -s ed80d1 -d 227832
  rebasing 2:ed80d1d38c22 "mv b d"
  merging b and d to d
  rebasing 3:44e133f71df3 "mv d e, mod a" (tip)
  merging c and a to c
  merging d and e to e
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/ed80d1d38c22-0be1aa5e-backup.hg (glob)
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: dd36053cfd7e66b8204c60270c29acc469dfac54
  |   desc: mv d e, mod a
  o  changeset: f1339f00f4e2201cf4430da19c09ab621ba0230f
  |   desc: mv b d
  o  changeset: 2278324ad33018f045aa853237766854f431307a
  |   desc: mv a c, mod b
  o  changeset: cc218bc7593246156e761e5477a5db40e26aabde
      desc: added a b
  $ ls
  c
  e
  $ cat c
  foo
  bar
  $ cat e
  bar
  foo
  $ hg status -C --change .
  M c
  A e
    d
  R d
  $ hg update -q .^
  $ ls
  c
  d
  $ hg status -C --change .
  A d
    b
  R b



FOURTH TEST

Cases tested:
Branch rebase

.              a f
.               :
.              a e
.               :  --->rebase source
.     c b      a d
.      :        :
.     a b  ....


Setup repo

  $ hg init repo
  $ cd repo
  $ echo 'foo' > a
  $ echo 'bar' > b
  $ hg add a b
  $ hg commit -m "added a b"
  $ hg mv a c
  $ hg commit -m "mv a c"
  $ hg update -q .^
  $ hg mv b d
  $ hg commit -q -m "mv b d"
  $ hg mv d e
  $ hg commit -m "mv d e"
  $ hg mv e f
  $ hg commit -m "mv e f"
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: 387c04c77e69232652cf681ad2eef1f93d0c9e23
  |   desc: mv e f
  o  changeset: 153612db400ba890a85460f1d6aaa8c73ef6e7d6
  |   desc: mv d e
  o  changeset: ed80d1d38c22b832268f774f8a71f8d4dd6625e2
  |   desc: mv b d
  | o  changeset: 924ff7a09e4cbf4d4c5cdd0c1ef4cb665c17188c
  |/    desc: mv a c
  o  changeset: cc218bc7593246156e761e5477a5db40e26aabde
      desc: added a b

Rebase

  $ hg rebase -s 153612 -d 924ff7
  rebasing 3:153612db400b "mv d e"
  rebasing 4:387c04c77e69 "mv e f" (tip)
  saved backup bundle to $TESTTMP/repo/repo/.hg/strip-backup/153612db400b-1ca39551-backup.hg (glob)
  $ hg log -G -T 'changeset: {node}\n desc: {desc}\n'
  @  changeset: b819c5a5ef7911dc41b3d9f866656f61b189ffcf
  |   desc: mv e f
  o  changeset: 6d1f4a69edb9498a968d87ffe53635e0a6a73506
  |   desc: mv d e
  | o  changeset: ed80d1d38c22b832268f774f8a71f8d4dd6625e2
  | |   desc: mv b d
  o |  changeset: 924ff7a09e4cbf4d4c5cdd0c1ef4cb665c17188c
  |/    desc: mv a c
  o  changeset: cc218bc7593246156e761e5477a5db40e26aabde
      desc: added a b
  $ ls
  c
  f
  $ hg status -C --change .
  A f
    e
  R e
  $ hg update -q .^
  $ ls
  c
  e
  $ hg status -C --change .
  A e
    b
  R b
