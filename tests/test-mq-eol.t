
Test interactions between mq and patch.eol


  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=1" >> $HGRCPATH

  $ cat > makepatch.py <<EOF
  > f = file('eol.diff', 'wb')
  > w = f.write
  > w('test message\n')
  > w('diff --git a/a b/a\n')
  > w('--- a/a\n')
  > w('+++ b/a\n')
  > w('@@ -1,5 +1,5 @@\n')
  > w(' a\n')
  > w('-b\r\n')
  > w('+y\r\n')
  > w(' c\r\n')
  > w(' d\n')
  > w('-e\n')
  > w('\ No newline at end of file\n')
  > w('+z\r\n')
  > w('\ No newline at end of file\r\n')
  > EOF

  $ cat > cateol.py <<EOF
  > import sys
  > for line in file(sys.argv[1], 'rb'):
  >     line = line.replace('\r', '<CR>')
  >     line = line.replace('\n', '<LF>')
  >     print line
  > EOF

  $ hg init repo
  $ cd repo
  $ echo '\.diff' > .hgignore
  $ echo '\.rej' >> .hgignore


Test different --eol values

  $ python -c 'file("a", "wb").write("a\nb\nc\nd\ne")'
  $ hg ci -Am adda
  adding .hgignore
  adding a
  $ python ../makepatch.py
  $ hg qimport eol.diff
  adding eol.diff to series file

should fail in strict mode

  $ hg qpush
  applying eol.diff
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  errors during apply, please fix and refresh eol.diff
  [2]
  $ hg qpop
  popping eol.diff
  patch queue now empty

invalid eol

  $ hg --config patch.eol='LFCR' qpush
  applying eol.diff
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  errors during apply, please fix and refresh eol.diff
  [2]
  $ hg qpop
  popping eol.diff
  patch queue now empty

force LF

  $ hg --config patch.eol='CRLF' qpush
  applying eol.diff
  now at: eol.diff
  $ hg qrefresh
  $ python ../cateol.py .hg/patches/eol.diff
  test message<LF>
  <LF>
  diff -r 0d0bf99a8b7a a<LF>
  --- a/a<LF>
  +++ b/a<LF>
  @@ -1,5 +1,5 @@<LF>
  -a<LF>
  -b<LF>
  -c<LF>
  -d<LF>
  -e<LF>
  \ No newline at end of file<LF>
  +a<CR><LF>
  +y<CR><LF>
  +c<CR><LF>
  +d<CR><LF>
  +z<LF>
  \ No newline at end of file<LF>
  $ python ../cateol.py a
  a<CR><LF>
  y<CR><LF>
  c<CR><LF>
  d<CR><LF>
  z
  $ hg qpop
  popping eol.diff
  patch queue now empty

push again forcing LF and compare revisions

  $ hg --config patch.eol='CRLF' qpush
  applying eol.diff
  now at: eol.diff
  $ python ../cateol.py a
  a<CR><LF>
  y<CR><LF>
  c<CR><LF>
  d<CR><LF>
  z
  $ hg qpop
  popping eol.diff
  patch queue now empty

push again without LF and compare revisions

  $ hg qpush
  applying eol.diff
  now at: eol.diff
  $ python ../cateol.py a
  a<CR><LF>
  y<CR><LF>
  c<CR><LF>
  d<CR><LF>
  z
  $ hg qpop
  popping eol.diff
  patch queue now empty
  $ cd ..


Test .rej file EOL are left unchanged

  $ hg init testeol
  $ cd testeol
  $ python -c "file('a', 'wb').write('1\r\n2\r\n3\r\n4')"
  $ hg ci -Am adda
  adding a
  $ python -c "file('a', 'wb').write('1\r\n2\r\n33\r\n4')"
  $ hg qnew patch1
  $ hg qpop
  popping patch1
  patch queue now empty
  $ python -c "file('a', 'wb').write('1\r\n22\r\n33\r\n4')"
  $ hg ci -m changea

  $ hg --config 'patch.eol=LF' qpush
  applying patch1
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  errors during apply, please fix and refresh patch1
  [2]
  $ hg qpop
  popping patch1
  patch queue now empty
  $ cat a.rej
  --- a
  +++ a
  @@ -1,4 +1,4 @@
   1\r (esc)
   2\r (esc)
  -3\r (esc)
  +33\r (esc)
   4
  \ No newline at end of file

  $ hg --config 'patch.eol=auto' qpush
  applying patch1
  patching file a
  Hunk #1 FAILED at 0
  1 out of 1 hunks FAILED -- saving rejects to file a.rej
  patch failed, unable to continue (try -v)
  patch failed, rejects left in working dir
  errors during apply, please fix and refresh patch1
  [2]
  $ hg qpop
  popping patch1
  patch queue now empty
  $ cat a.rej
  --- a
  +++ a
  @@ -1,4 +1,4 @@
   1\r (esc)
   2\r (esc)
  -3\r (esc)
  +33\r (esc)
   4
  \ No newline at end of file
  $ cd ..
