  $ cat > makepatch.py <<EOF
  > f = file('eol.diff', 'wb')
  > w = f.write
  > w('test message\n')
  > w('diff --git a/a b/a\n')
  > w('--- a/a\n')
  > w('+++ b/a\n')
  > w('@@ -1,5 +1,5 @@\n')
  > w(' a\n')
  > w('-bbb\r\n')
  > w('+yyyy\r\n')
  > w(' cc\r\n')
  > w(' \n')
  > w(' d\n')
  > w('-e\n')
  > w('\ No newline at end of file\n')
  > w('+z\r\n')
  > w('\ No newline at end of file\r\n')
  > EOF

  $ hg init repo
  $ cd repo
  $ echo '\.diff' > .hgignore


Test different --eol values

  $ python -c 'file("a", "wb").write("a\nbbb\ncc\n\nd\ne")'
  $ hg ci -Am adda
  adding .hgignore
  adding a
  $ python ../makepatch.py


invalid eol

  $ hg --config patch.eol='LFCR' import eol.diff
  applying eol.diff
  abort: unsupported line endings type: LFCR
  [255]
  $ hg revert -a


force LF

  $ hg --traceback --config patch.eol='LF' import eol.diff
  applying eol.diff
  $ cat a
  a
  yyyy
  cc
  
  d
  e (no-eol)
  $ hg st


force CRLF

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --traceback --config patch.eol='CRLF' import eol.diff
  applying eol.diff
  $ cat a
  a\r (esc)
  yyyy\r (esc)
  cc\r (esc)
  \r (esc)
  d\r (esc)
  e (no-eol)
  $ hg st


auto EOL on LF file

  $ hg up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --traceback --config patch.eol='auto' import eol.diff
  applying eol.diff
  $ cat a
  a
  yyyy
  cc
  
  d
  e (no-eol)
  $ hg st


auto EOL on CRLF file

  $ python -c 'file("a", "wb").write("a\r\nbbb\r\ncc\r\n\r\nd\r\ne")'
  $ hg commit -m 'switch EOLs in a'
  $ hg --traceback --config patch.eol='auto' import eol.diff
  applying eol.diff
  $ cat a
  a\r (esc)
  yyyy\r (esc)
  cc\r (esc)
  \r (esc)
  d\r (esc)
  e (no-eol)
  $ hg st


auto EOL on new file or source without any EOL

  $ python -c 'file("noeol", "wb").write("noeol")'
  $ hg add noeol
  $ hg commit -m 'add noeol'
  $ python -c 'file("noeol", "wb").write("noeol\r\nnoeol\n")'
  $ python -c 'file("neweol", "wb").write("neweol\nneweol\r\n")'
  $ hg add neweol
  $ hg diff --git > noeol.diff
  $ hg revert --no-backup noeol neweol
  $ rm neweol
  $ hg --traceback --config patch.eol='auto' import -m noeol noeol.diff
  applying noeol.diff
  $ cat noeol
  noeol\r (esc)
  noeol
  $ cat neweol
  neweol
  neweol\r (esc)
  $ hg st


Test --eol and binary patches

  $ python -c 'file("b", "wb").write("a\x00\nb\r\nd")'
  $ hg ci -Am addb
  adding b
  $ python -c 'file("b", "wb").write("a\x00\nc\r\nd")'
  $ hg diff --git > bin.diff
  $ hg revert --no-backup b

binary patch with --eol

  $ hg import --config patch.eol='CRLF' -m changeb bin.diff
  applying bin.diff
  $ cat b
  a\x00 (esc)
  c\r (esc)
  d (no-eol)
  $ hg st
  $ cd ..
