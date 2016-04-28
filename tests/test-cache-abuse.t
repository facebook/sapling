Enable obsolete markers

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=createmarkers
  > [phases]
  > publish=False
  > EOF

Build a repo with some cacheable bits:

  $ hg init a
  $ cd a

  $ echo a > a
  $ hg ci -qAm0
  $ hg tag t1
  $ hg book -i bk1

  $ hg branch -q b2
  $ hg ci -Am1
  $ hg tag t2

  $ echo dumb > dumb
  $ hg ci -qAmdumb
  $ hg debugobsolete b1174d11b69e63cb0c5726621a43c859f0858d7f

  $ hg phase -pr t1
  $ hg phase -fsr t2

Make a helper function to check cache damage invariants:

- command output shouldn't change
- cache should be present after first use
- corruption/repair should be silent (no exceptions or warnings)
- cache should survive deletion, overwrite, and append
- unreadable / unwriteable caches should be ignored
- cache should be rebuilt after corruption

  $ damage() {
  >  CMD=$1
  >  CACHE=.hg/cache/$2
  >  CLEAN=$3
  >  hg $CMD > before
  >  test -f $CACHE || echo "not present"
  >  echo bad > $CACHE
  >  test -z "$CLEAN" || $CLEAN
  >  hg $CMD > after
  >  diff -u before after || echo "*** overwrite corruption"
  >  echo corruption >> $CACHE
  >  test -z "$CLEAN" || $CLEAN
  >  hg $CMD > after
  >  diff -u before after || echo "*** append corruption"
  >  rm $CACHE
  >  mkdir $CACHE
  >  test -z "$CLEAN" || $CLEAN
  >  hg $CMD > after
  >  diff -u before after || echo "*** read-only corruption"
  >  test -d $CACHE || echo "*** directory clobbered"
  >  rmdir $CACHE
  >  test -z "$CLEAN" || $CLEAN
  >  hg $CMD > after
  >  diff -u before after || echo "*** missing corruption"
  >  test -f $CACHE || echo "not rebuilt"
  > }

Beat up tags caches:

  $ damage "tags --hidden" tags2
  $ damage tags tags2-visible
  $ damage "tag -f t3" hgtagsfnodes1

Beat up hidden cache:

  $ damage log hidden

Beat up branch caches:

  $ damage branches branch2-base "rm .hg/cache/branch2-[vs]*"
  $ damage branches branch2-served "rm .hg/cache/branch2-[bv]*"
  $ damage branches branch2-visible
  $ damage "log -r branch(.)" rbc-names-v1
  $ damage "log -r branch(default)" rbc-names-v1
  $ damage "log -r branch(b2)" rbc-revs-v1

We currently can't detect an rbc cache with unknown names:

  $ damage "log -qr branch(b2)" rbc-names-v1
  --- before	* (glob)
  +++ after	* (glob)
  @@ -1,8 +0,0 @@
  -2:5fb7d38b9dc4
  -3:60b597ffdafa
  -4:b1174d11b69e
  -5:6354685872c0
  -6:5ebc725f1bef
  -7:7b76eec2f273
  -8:ef3428d9d644
  -9:ba7a936bc03c
  *** append corruption
