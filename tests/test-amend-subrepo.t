#testcases obsstore-off obsstore-on

  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > amend =
  > EOF

#if obsstore-on
  $ cat << EOF >> $HGRCPATH
  > [experimental]
  > evolution.createmarkers = True
  > EOF
#endif

Prepare parent repo
-------------------

  $ hg init r
  $ cd r

  $ echo a > a
  $ hg ci -Am0
  adding a

Link first subrepo
------------------

  $ echo 's = s' >> .hgsub
  $ hg add .hgsub
  $ hg init s

amend without .hgsub

  $ hg amend s
  abort: can't commit subrepos without .hgsub
  [255]

amend with subrepo

  $ hg amend
  saved backup bundle to * (glob) (obsstore-off !)
  $ hg status --change .
  A .hgsub
  A .hgsubstate
  A a
  $ cat .hgsubstate
  0000000000000000000000000000000000000000 s

Update subrepo
--------------

add new commit to be amended

  $ echo a >> a
  $ hg ci -m1

amend with dirty subrepo

  $ echo a >> s/a
  $ hg add -R s
  adding s/a (glob)
  $ hg amend
  abort: uncommitted changes in subrepository "s"
  (use --subrepos for recursive commit)
  [255]

amend with modified subrepo

  $ hg ci -R s -m0
  $ hg amend
  saved backup bundle to * (glob) (obsstore-off !)
  $ hg status --change .
  M .hgsubstate
  M a
  $ cat .hgsubstate
  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac s

revert subrepo change

  $ hg up -R s -q null
  $ hg amend
  saved backup bundle to * (glob) (obsstore-off !)
  $ hg status --change .
  M a

Link another subrepo
--------------------

add new commit to be amended

  $ echo b >> b
  $ hg ci -qAm2

also checks if non-subrepo change is included

  $ echo a >> a

amend with another subrepo

  $ hg init t
  $ echo b >> t/b
  $ hg ci -R t -Am0
  adding b
  $ echo 't = t' >> .hgsub
  $ hg amend
  saved backup bundle to * (glob) (obsstore-off !)
  $ hg status --change .
  M .hgsub
  M .hgsubstate
  M a
  A b
  $ cat .hgsubstate
  0000000000000000000000000000000000000000 s
  bfb1a4fb358498a9533dabf4f2043d94162f1fcd t

Unlink one subrepo
------------------

add new commit to be amended

  $ echo a >> a
  $ hg ci -m3

amend with one subrepo dropped

  $ echo 't = t' > .hgsub
  $ hg amend
  saved backup bundle to * (glob) (obsstore-off !)
  $ hg status --change .
  M .hgsub
  M .hgsubstate
  M a
  $ cat .hgsubstate
  bfb1a4fb358498a9533dabf4f2043d94162f1fcd t

Unlink subrepos completely
--------------------------

add new commit to be amended

  $ echo a >> a
  $ hg ci -m3

amend with .hgsub removed

  $ hg rm .hgsub
  $ hg amend
  saved backup bundle to * (glob) (obsstore-off !)
  $ hg status --change .
  M a
  R .hgsub
  R .hgsubstate

  $ cd ..
