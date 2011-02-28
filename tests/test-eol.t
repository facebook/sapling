Test EOL extension

  $ cat > $HGRCPATH <<EOF
  > [diff]
  > git = True
  > EOF

Set up helpers

  $ cat > switch-eol.py <<EOF
  > import sys
  > try:
  >     import os, msvcrt
  >     msvcrt.setmode(sys.stdin.fileno(), os.O_BINARY)
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  > except ImportError:
  >     pass
  > (old, new) = sys.argv[1] == 'LF' and ('\n', '\r\n') or ('\r\n', '\n')
  > print "%% switching encoding from %r to %r" % (old, new)
  > for path in sys.argv[2:]:
  >     data = file(path, 'rb').read()
  >     data = data.replace(old, new)
  >     file(path, 'wb').write(data)
  > EOF

  $ seteol () {
  >     if [ $1 = "LF" ]; then
  >         EOL='\n'
  >     else
  >         EOL='\r\n'
  >     fi
  > }

  $ makerepo () {
  >     seteol $1
  >     echo "% setup $1 repository"
  >     hg init repo
  >     cd repo
  >     cat > .hgeol <<EOF
  > [repository]
  > native = $1
  > [patterns]
  > mixed.txt = BIN
  > **.txt = native
  > EOF
  >     printf "first${EOL}second${EOL}third${EOL}" > a.txt
  >     hg commit --addremove -m 'checkin'
  >     echo
  >     cd ..
  > }

  $ dotest () {
  >     seteol $1
  >     echo "% hg clone repo repo-$1"
  >     hg clone --noupdate repo repo-$1
  >     cd repo-$1
  >     cat > .hg/hgrc <<EOF
  > [extensions]
  > eol =
  > [eol]
  > native = $1
  > EOF
  >     hg update
  >     echo '% a.txt'
  >     cat a.txt
  >     echo '% hg cat a.txt'
  >     hg cat a.txt
  >     printf "fourth${EOL}" >> a.txt
  >     echo '% a.txt'
  >     cat a.txt
  >     hg diff
  >     python ../switch-eol.py $1 a.txt
  >     echo '% hg diff only reports a single changed line:'
  >     hg diff
  >     echo "% reverting back to $1 format"
  >     hg revert a.txt
  >     cat a.txt
  >     printf "first\r\nsecond\n" > mixed.txt
  >     hg add mixed.txt
  >     echo "% hg commit of inconsistent .txt file marked as binary (should work)"
  >     hg commit -m 'binary file'
  >     echo "% hg commit of inconsistent .txt file marked as native (should fail)"
  >     printf "first\nsecond\r\nthird\nfourth\r\n" > a.txt
  >     hg commit -m 'inconsistent file'
  >     echo "% hg commit --config eol.only-consistent=False (should work)"
  >     hg commit --config eol.only-consistent=False -m 'inconsistent file'
  >     echo "% hg commit of binary .txt file marked as native (binary files always okay)"
  >     printf "first${EOL}\0${EOL}third${EOL}" > a.txt
  >     hg commit -m 'binary file'
  >     cd ..
  >     rm -r repo-$1
  > }

  $ makemixedrepo () {
  >     echo
  >     echo "# setup $1 repository"
  >     hg init mixed
  >     cd mixed
  >     printf "foo\r\nbar\r\nbaz\r\n" > win.txt
  >     printf "foo\nbar\nbaz\n" > unix.txt
  >     #printf "foo\r\nbar\nbaz\r\n" > mixed.txt
  >     hg commit --addremove -m 'created mixed files'
  >     echo "# setting repository-native EOLs to $1"
  >     cat > .hgeol <<EOF
  > [repository]
  > native = $1
  > [patterns]
  > **.txt = native
  > EOF
  >     hg commit --addremove -m 'added .hgeol'
  >     cd ..
  > }

  $ testmixed () {
  >     echo
  >     echo "% hg clone mixed mixed-$1"
  >     hg clone mixed mixed-$1
  >     cd mixed-$1
  >     echo '% hg status (eol extension not yet activated)'
  >     hg status
  >     cat > .hg/hgrc <<EOF
  > [extensions]
  > eol =
  > [eol]
  > native = $1
  > EOF
  >     echo '% hg status (eol activated)'
  >     hg status
  >     echo '% hg commit'
  >     hg commit -m 'synchronized EOLs'
  >     echo '% hg status'
  >     hg status
  >     cd ..
  >     rm -r mixed-$1
  > }

Basic tests

  $ makerepo LF
  % setup LF repository
  adding .hgeol
  adding a.txt
  
  $ dotest LF
  % hg clone repo repo-LF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % a.txt
  first
  second
  third
  % hg cat a.txt
  first
  second
  third
  % a.txt
  first
  second
  third
  fourth
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first
   second
   third
  +fourth
  % switching encoding from '\n' to '\r\n'
  % hg diff only reports a single changed line:
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first
   second
   third
  +fourth
  % reverting back to LF format
  first
  second
  third
  % hg commit of inconsistent .txt file marked as binary (should work)
  % hg commit of inconsistent .txt file marked as native (should fail)
  abort: inconsistent newline style in a.txt
  
  % hg commit --config eol.only-consistent=False (should work)
  % hg commit of binary .txt file marked as native (binary files always okay)
  $ dotest CRLF
  % hg clone repo repo-CRLF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg cat a.txt
  first
  second
  third
  % a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  fourth\r (esc)
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first
   second
   third
  +fourth
  % switching encoding from '\r\n' to '\n'
  % hg diff only reports a single changed line:
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first
   second
   third
  +fourth
  % reverting back to CRLF format
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg commit of inconsistent .txt file marked as binary (should work)
  % hg commit of inconsistent .txt file marked as native (should fail)
  abort: inconsistent newline style in a.txt
  
  % hg commit --config eol.only-consistent=False (should work)
  % hg commit of binary .txt file marked as native (binary files always okay)
  $ rm -r repo
  $ makerepo CRLF
  % setup CRLF repository
  adding .hgeol
  adding a.txt
  
  $ dotest LF
  % hg clone repo repo-LF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % a.txt
  first
  second
  third
  % hg cat a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % a.txt
  first
  second
  third
  fourth
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first\r (esc)
   second\r (esc)
   third\r (esc)
  +fourth\r (esc)
  % switching encoding from '\n' to '\r\n'
  % hg diff only reports a single changed line:
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first\r (esc)
   second\r (esc)
   third\r (esc)
  +fourth\r (esc)
  % reverting back to LF format
  first
  second
  third
  % hg commit of inconsistent .txt file marked as binary (should work)
  % hg commit of inconsistent .txt file marked as native (should fail)
  abort: inconsistent newline style in a.txt
  
  % hg commit --config eol.only-consistent=False (should work)
  % hg commit of binary .txt file marked as native (binary files always okay)
  $ dotest CRLF
  % hg clone repo repo-CRLF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg cat a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % a.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  fourth\r (esc)
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first\r (esc)
   second\r (esc)
   third\r (esc)
  +fourth\r (esc)
  % switching encoding from '\r\n' to '\n'
  % hg diff only reports a single changed line:
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,4 @@
   first\r (esc)
   second\r (esc)
   third\r (esc)
  +fourth\r (esc)
  % reverting back to CRLF format
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg commit of inconsistent .txt file marked as binary (should work)
  % hg commit of inconsistent .txt file marked as native (should fail)
  abort: inconsistent newline style in a.txt
  
  % hg commit --config eol.only-consistent=False (should work)
  % hg commit of binary .txt file marked as native (binary files always okay)
  $ rm -r repo

Mixed tests

  $ makemixedrepo LF
  
  # setup LF repository
  adding unix.txt
  adding win.txt
  # setting repository-native EOLs to LF
  adding .hgeol
  $ testmixed LF
  
  % hg clone mixed mixed-LF
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % hg status (eol extension not yet activated)
  % hg status (eol activated)
  M win.txt
  % hg commit
  % hg status
  $ testmixed CRLF
  
  % hg clone mixed mixed-CRLF
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % hg status (eol extension not yet activated)
  % hg status (eol activated)
  M win.txt
  % hg commit
  % hg status
  $ rm -r mixed
  $ makemixedrepo CRLF
  
  # setup CRLF repository
  adding unix.txt
  adding win.txt
  # setting repository-native EOLs to CRLF
  adding .hgeol
  $ testmixed LF
  
  % hg clone mixed mixed-LF
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % hg status (eol extension not yet activated)
  % hg status (eol activated)
  M unix.txt
  % hg commit
  % hg status
  $ testmixed CRLF
  
  % hg clone mixed mixed-CRLF
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % hg status (eol extension not yet activated)
  % hg status (eol activated)
  M unix.txt
  % hg commit
  % hg status
  $ rm -r mixed

Test issue2569 -- eol extension takes write lock on reading:

  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'eol =' >> $HGRCPATH
  $ hg init repo
  $ cd repo
  $ touch .hgeol
  $ hg status
  ? .hgeol
  $ chmod -R -w .hg
  $ sleep 1
  $ touch .hgeol
  $ hg status --traceback
  ? .hgeol
  $ cd ..

Test cleverencode: and cleverdecode: aliases for win32text extension

  $ echo '[encode]' >> $HGRCPATH
  $ echo '**.txt = cleverencode:' >> $HGRCPATH
  $ echo '[decode]' >> $HGRCPATH
  $ echo '**.txt = cleverdecode:' >> $HGRCPATH

  $ hg init win32compat
  $ cd win32compat
  $ printf "foo\r\nbar\r\nbaz\r\n" > win.txt
  $ printf "foo\nbar\nbaz\n" > unix.txt
  $ hg add
  adding unix.txt
  adding win.txt
  $ hg commit -m checkin

Check that both files have LF line-endings in the repository:

  $ hg cat win.txt
  foo
  bar
  baz
  $ hg cat unix.txt
  foo
  bar
  baz

Test handling of a broken .hgeol file:

  $ touch .hgeol
  $ hg add .hgeol
  $ hg commit -m 'clean version'
  $ echo "bad" > .hgeol
  $ hg status
  warning: ignoring .hgeol file due to parse error at .hgeol:1: bad
  M .hgeol
  $ hg revert .hgeol
  warning: ignoring .hgeol file due to parse error at .hgeol:1: bad
  $ hg status
  ? .hgeol.orig
