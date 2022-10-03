#chg-compatible

  $ setconfig workingcopy.ruststatus=False
  $ setconfig status.use-rust=False workingcopy.use-rust=False

  $ cat > makepatch.py <<EOF
  > f = open('eol.diff', 'wb')
  > w = f.write
  > _ = w(b'test message\n')
  > _ = w(b'diff --git a/a b/a\n')
  > _ = w(b'--- a/a\n')
  > _ = w(b'+++ b/a\n')
  > _ = w(b'@@ -1,5 +1,5 @@\n')
  > _ = w(b' a\n')
  > _ = w(b'-bbb\r\n')
  > _ = w(b'+yyyy\r\n')
  > _ = w(b' cc\r\n')
  > _ = w(b' \n')
  > _ = w(b' d\n')
  > _ = w(b'-e\n')
  > _ = w(b'\ No newline at end of file\n')
  > _ = w(b'+z\r\n')
  > _ = w(b'\ No newline at end of file\r\n')
  > EOF

  $ hg init repo
  $ cd repo
  $ echo '*\.diff' > .gitignore


Test different --eol values

  $ printf "a\nbbb\ncc\n\nd\ne" > a
  $ hg ci -Am adda
  adding .gitignore
  adding a
  $ $PYTHON ../makepatch.py


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

  $ hg up -C 'desc(adda)'
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

  $ hg up -C 'desc(adda)'
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

  $ printf "a\r\nbbb\r\ncc\r\n\r\nd\r\ne" > a
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

  $ printf "noeol" > noeol
  $ hg add noeol
  $ hg commit -m 'add noeol'
  $ printf "noeol\r\nnoeol\n" > noeol
  $ printf "neweol\nneweol\r\n" > neweol
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

  $ printf "a\x00\nb\r\nd" > b
  $ hg ci -Am addb
  adding b
  $ printf "a\x00\nc\r\nd" > b
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
