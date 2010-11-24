Test character encoding

  $ hg init t
  $ cd t

we need a repo with some legacy latin-1 changesets

  $ hg unbundle $TESTDIR/legacy-encoding.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg co
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ python << EOF
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
  abort: decoding near ' encoded: \xe9': 'ascii' codec can't decode byte 0xe9 in position 20: ordinal not in range(128)! (esc)
  [255]

these should work

  $ echo "latin-1" > a
  $ HGENCODING=latin-1 hg ci -l latin-1
  $ echo "utf-8" > a
  $ HGENCODING=utf-8 hg ci -l utf-8
  $ HGENCODING=latin-1 hg tag `cat latin-1-tag`
  $ HGENCODING=latin-1 hg branch `cat latin-1-tag`
  marked working directory as branch \xe9 (esc)
  $ HGENCODING=latin-1 hg ci -m 'latin1 branch'
  $ rm .hg/branch

hg log (ascii)

  $ hg --encoding ascii log
  changeset:   5:093c6077d1c8
  branch:      ?
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin1 branch
  
  changeset:   4:94db611b4196
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag ? for changeset ca661e7520de
  
  changeset:   3:ca661e7520de
  tag:         ?
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
  changeset:   5:093c6077d1c8
  branch:      \xe9 (esc)
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin1 branch
  
  changeset:   4:94db611b4196
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag \xe9 for changeset ca661e7520de (esc)
  
  changeset:   3:ca661e7520de
  tag:         \xe9 (esc)
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
  changeset:   5:093c6077d1c8
  branch:      \xc3\xa9 (esc)
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin1 branch
  
  changeset:   4:94db611b4196
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag \xc3\xa9 for changeset ca661e7520de (esc)
  
  changeset:   3:ca661e7520de
  tag:         \xc3\xa9 (esc)
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
  

hg tags (ascii)

  $ HGENCODING=ascii hg tags
  tip                                5:093c6077d1c8
  ?                                  3:ca661e7520de

hg tags (latin-1)

  $ HGENCODING=latin-1 hg tags
  tip                                5:093c6077d1c8
  \xe9                                  3:ca661e7520de (esc)

hg tags (utf-8)

  $ HGENCODING=utf-8 hg tags
  tip                                5:093c6077d1c8
  \xc3\xa9                                  3:ca661e7520de (esc)

hg branches (ascii)

  $ HGENCODING=ascii hg branches
  ?                              5:093c6077d1c8
  default                        4:94db611b4196 (inactive)

hg branches (latin-1)

  $ HGENCODING=latin-1 hg branches
  \xe9                              5:093c6077d1c8 (esc)
  default                        4:94db611b4196 (inactive)

hg branches (utf-8)

  $ HGENCODING=utf-8 hg branches
  \xc3\xa9                              5:093c6077d1c8 (esc)
  default                        4:94db611b4196 (inactive)
  $ echo '[ui]' >> .hg/hgrc
  $ echo 'fallbackencoding = koi8-r' >> .hg/hgrc

hg log (utf-8)

  $ HGENCODING=utf-8 hg log
  changeset:   5:093c6077d1c8
  branch:      \xc3\xa9 (esc)
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin1 branch
  
  changeset:   4:94db611b4196
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag \xc3\xa9 for changeset ca661e7520de (esc)
  
  changeset:   3:ca661e7520de
  tag:         \xc3\xa9 (esc)
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
  summary:     koi8-r: \xd1\x80\xd1\x82\xd1\x83\xd1\x82\xd1\x8c = u'\\u0440\\u0442\\u0443\\u0442\\u044c' (esc)
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': \xd0\x98 = u'\\xe9' (esc)
  

hg log (dolphin)

  $ HGENCODING=dolphin hg log
  abort: unknown encoding: dolphin, please check your locale settings
  [255]
  $ HGENCODING=ascii hg branch `cat latin-1-tag`
  abort: decoding near '\xe9': 'ascii' codec can't decode byte 0xe9 in position 0: ordinal not in range(128)! (esc)
  [255]
  $ cp latin-1-tag .hg/branch
  $ HGENCODING=latin-1 hg ci -m 'auto-promote legacy name'
