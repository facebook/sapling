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
  abort: decoding near ' encoded: È': 'ascii' codec can't decode byte 0xe9 in position 20: ordinal not in range(128)!
  [255]

these should work

  $ echo "latin-1" > a
  $ HGENCODING=latin-1 hg ci -l latin-1
  $ echo "utf-8" > a
  $ HGENCODING=utf-8 hg ci -l utf-8
  $ HGENCODING=latin-1 hg tag `cat latin-1-tag`
  $ HGENCODING=latin-1 hg branch `cat latin-1-tag`
  marked working directory as branch È
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
  branch:      È
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin1 branch
  
  changeset:   4:94db611b4196
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag È for changeset ca661e7520de
  
  changeset:   3:ca661e7520de
  tag:         È
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     utf-8 e' encoded: È
  
  changeset:   2:650c6f3d55dd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin-1 e' encoded: È
  
  changeset:   1:0e5b7e3f9c4a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     koi8-r: “‘’‘ÿ = u'\u0440\u0442\u0443\u0442\u044c'
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': È = u'\xe9'
  

hg log (utf-8)

  $ hg --encoding utf-8 log
  changeset:   5:093c6077d1c8
  branch:      √©
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin1 branch
  
  changeset:   4:94db611b4196
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag √© for changeset ca661e7520de
  
  changeset:   3:ca661e7520de
  tag:         √©
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     utf-8 e' encoded: √©
  
  changeset:   2:650c6f3d55dd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin-1 e' encoded: √©
  
  changeset:   1:0e5b7e3f9c4a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     koi8-r: √í√î√ï√î√ò = u'\u0440\u0442\u0443\u0442\u044c'
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': √© = u'\xe9'
  

hg tags (ascii)

  $ HGENCODING=ascii hg tags
  tip                                5:093c6077d1c8
  ?                                  3:ca661e7520de

hg tags (latin-1)

  $ HGENCODING=latin-1 hg tags
  tip                                5:093c6077d1c8
  È                                  3:ca661e7520de

hg tags (utf-8)

  $ HGENCODING=utf-8 hg tags
  tip                                5:093c6077d1c8
  √©                                  3:ca661e7520de

hg branches (ascii)

  $ HGENCODING=ascii hg branches
  ?                              5:093c6077d1c8
  default                        4:94db611b4196 (inactive)

hg branches (latin-1)

  $ HGENCODING=latin-1 hg branches
  È                              5:093c6077d1c8
  default                        4:94db611b4196 (inactive)

hg branches (utf-8)

  $ HGENCODING=utf-8 hg branches
  √©                              5:093c6077d1c8
  default                        4:94db611b4196 (inactive)
  $ echo '[ui]' >> .hg/hgrc
  $ echo 'fallbackencoding = koi8-r' >> .hg/hgrc

hg log (utf-8)

  $ HGENCODING=utf-8 hg log
  changeset:   5:093c6077d1c8
  branch:      √©
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin1 branch
  
  changeset:   4:94db611b4196
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag √© for changeset ca661e7520de
  
  changeset:   3:ca661e7520de
  tag:         √©
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     utf-8 e' encoded: √©
  
  changeset:   2:650c6f3d55dd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     latin-1 e' encoded: √©
  
  changeset:   1:0e5b7e3f9c4a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     koi8-r: —Ä—Ç—É—Ç—å = u'\u0440\u0442\u0443\u0442\u044c'
  
  changeset:   0:1e78a93102a3
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     latin-1 e': –ò = u'\xe9'
  

hg log (dolphin)

  $ HGENCODING=dolphin hg log
  abort: unknown encoding: dolphin, please check your locale settings
  [255]
  $ HGENCODING=ascii hg branch `cat latin-1-tag`
  abort: decoding near 'È': 'ascii' codec can't decode byte 0xe9 in position 0: ordinal not in range(128)!
  [255]
  $ cp latin-1-tag .hg/branch
  $ HGENCODING=latin-1 hg ci -m 'should fail'
  abort: branch name not in UTF-8!
  [255]
