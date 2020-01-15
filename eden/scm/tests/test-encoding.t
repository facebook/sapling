Test character encoding

  $ hg init t
  $ cd t

we need a repo with some legacy latin-1 changesets

  $ hg unbundle "$TESTDIR/bundles/legacy-encoding.hg"
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  $ hg co
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ $PYTHON << EOF
  > f = file('latin-1', 'w'); f.write("latin-1 e' encoded: \xe9"); f.close()
  > f = file('utf-8', 'w'); f.write("utf-8 e' encoded: \xc3\xa9"); f.close()
  > f = file('latin-1-tag', 'w'); f.write("\xe9"); f.close()
  > EOF

should fail with encoding error

  $ echo "plain old ascii" > a
  $ hg st
  M a
  ? latin-1
  ? latin-1-tag
  ? utf-8
  $ HGENCODING=ascii hg ci -l latin-1
  transaction abort!
  rollback completed
  abort: decoding near ' encoded: \xe9': 'utf8' codec can't decode byte 0xe9 in position 20: unexpected end of data! (esc)
  [255]

these should work

  $ echo "latin-1" > a
  $ HGENCODING=latin-1 hg ci -l latin-1
  $ echo "utf-8" > a
  $ HGENCODING=utf-8 hg ci -l utf-8

hg log (ascii)

  $ hg --encoding ascii log
  changeset:   3:ca661e7520de
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     utf-8 e' encoded: ?
  
  changeset:   2:650c6f3d55dd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin-1 e' encoded: ?
  
  changeset:   1:0e5b7e3f9c4a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     koi8-r: ????? = u'\u0440\u0442\u0443\u0442\u044c'
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': ? = u'\xe9'
  

hg log (latin-1)

  $ hg --encoding latin-1 log
  changeset:   3:ca661e7520de
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     utf-8 e' encoded: \xe9 (esc)
  
  changeset:   2:650c6f3d55dd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin-1 e' encoded: \xe9 (esc)
  
  changeset:   1:0e5b7e3f9c4a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     koi8-r: \xd2\xd4\xd5\xd4\xd8 = u'\\u0440\\u0442\\u0443\\u0442\\u044c' (esc)
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': \xe9 = u'\\xe9' (esc)
  

hg log (utf-8)

  $ hg --encoding utf-8 log
  changeset:   3:ca661e7520de
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     utf-8 e' encoded: \xc3\xa9 (esc)
  
  changeset:   2:650c6f3d55dd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin-1 e' encoded: \xc3\xa9 (esc)
  
  changeset:   1:0e5b7e3f9c4a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     koi8-r: \xc3\x92\xc3\x94\xc3\x95\xc3\x94\xc3\x98 = u'\\u0440\\u0442\\u0443\\u0442\\u044c' (esc)
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': \xc3\xa9 = u'\\xe9' (esc)
  

hg log (utf-8)

  $ HGENCODING=utf-8 hg log
  changeset:   3:ca661e7520de
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     utf-8 e' encoded: \xc3\xa9 (esc)
  
  changeset:   2:650c6f3d55dd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin-1 e' encoded: \xc3\xa9 (esc)
  
  changeset:   1:0e5b7e3f9c4a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     koi8-r: \xc3\x92\xc3\x94\xc3\x95\xc3\x94\xc3\x98 = u'\\u0440\\u0442\\u0443\\u0442\\u044c' (esc)
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': \xc3\xa9 = u'\\xe9' (esc)
  

hg log (dolphin)

  $ HGENCODING=dolphin hg log
  abort: unknown encoding: dolphin
  (please check your locale settings)
  [255]
  $ cp latin-1-tag .hg/branch
  $ HGENCODING=latin-1 hg ci -m 'auto-promote legacy name'

  $ cd ..

Test roundtrip encoding/decoding of utf8b for generated data

#if hypothesis

  >>> from hypothesishelpers import *
  >>> from edenscm.mercurial import encoding
  >>> roundtrips(st.binary(), encoding.fromutf8b, encoding.toutf8b)
  Round trip OK

#endif
