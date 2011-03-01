Test EOL patching

  $ cat >> $HGRCPATH <<EOF
  > [diff]
  > git = 1
  > EOF

Set up helpers

  $ seteol () {
  >     if [ $1 = "LF" ]; then
  >         EOL='\n'
  >     else
  >         EOL='\r\n'
  >     fi
  > }

  $ makerepo () {
  >     seteol $1
  >     echo
  >     echo "# ==== setup $1 repository ===="
  >     echo '% hg init'
  >     hg init repo
  >     cd repo
  >     cat > .hgeol <<EOF
  > [repository]
  > native = $1
  > [patterns]
  > unix.txt = LF
  > win.txt = CRLF
  > **.txt = native
  > EOF
  >     printf "first\r\nsecond\r\nthird\r\n" > win.txt
  >     printf "first\nsecond\nthird\n" > unix.txt
  >     printf "first${EOL}second${EOL}third${EOL}" > native.txt
  >     hg commit --addremove -m 'checkin'
  >     cd ..
  > }

  $ dotest () {
  >     seteol $1
  >     echo
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
  >     echo '% native.txt'
  >     cat native.txt
  >     echo '% unix.txt'
  >     cat unix.txt
  >     echo '% win.txt'
  >     cat win.txt
  >     printf "first${EOL}third${EOL}" > native.txt
  >     printf "first\r\nthird\r\n" > win.txt
  >     printf "first\nthird\n" > unix.txt
  >     echo '% hg diff'
  >     hg diff > p
  >     cat p
  >     echo '% hg revert'
  >     hg revert --all
  >     echo '% hg import'
  >     hg import -m 'patch' p
  >     echo '% native.txt'
  >     cat native.txt
  >     echo '% unix.txt'
  >     cat unix.txt
  >     echo '% win.txt'
  >     cat win.txt
  >     echo '% hg diff -c tip'
  >     hg diff -c tip
  >     cd ..
  >     rm -r repo-$1
  > }

Run tests

  $ makerepo LF
  
  # ==== setup LF repository ====
  % hg init
  adding .hgeol
  adding native.txt
  adding unix.txt
  adding win.txt
  $ dotest LF
  
  % hg clone repo repo-LF
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % native.txt
  first
  second
  third
  % unix.txt
  first
  second
  third
  % win.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg diff
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  % hg revert
  reverting native.txt
  reverting unix.txt
  reverting win.txt
  % hg import
  applying p
  % native.txt
  first
  third
  % unix.txt
  first
  third
  % win.txt
  first\r (esc)
  third\r (esc)
  % hg diff -c tip
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  $ dotest CRLF
  
  % hg clone repo repo-CRLF
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % native.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % unix.txt
  first
  second
  third
  % win.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg diff
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  % hg revert
  reverting native.txt
  reverting unix.txt
  reverting win.txt
  % hg import
  applying p
  % native.txt
  first\r (esc)
  third\r (esc)
  % unix.txt
  first
  third
  % win.txt
  first\r (esc)
  third\r (esc)
  % hg diff -c tip
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  $ rm -r repo
  $ makerepo CRLF
  
  # ==== setup CRLF repository ====
  % hg init
  adding .hgeol
  adding native.txt
  adding unix.txt
  adding win.txt
  $ dotest LF
  
  % hg clone repo repo-LF
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % native.txt
  first
  second
  third
  % unix.txt
  first
  second
  third
  % win.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg diff
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  % hg revert
  reverting native.txt
  reverting unix.txt
  reverting win.txt
  % hg import
  applying p
  % native.txt
  first
  third
  % unix.txt
  first
  third
  % win.txt
  first\r (esc)
  third\r (esc)
  % hg diff -c tip
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  $ dotest CRLF
  
  % hg clone repo repo-CRLF
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % native.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % unix.txt
  first
  second
  third
  % win.txt
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % hg diff
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  % hg revert
  reverting native.txt
  reverting unix.txt
  reverting win.txt
  % hg import
  applying p
  % native.txt
  first\r (esc)
  third\r (esc)
  % unix.txt
  first
  third
  % win.txt
  first\r (esc)
  third\r (esc)
  % hg diff -c tip
  diff --git a/native.txt b/native.txt
  --- a/native.txt
  +++ b/native.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  diff --git a/unix.txt b/unix.txt
  --- a/unix.txt
  +++ b/unix.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  diff --git a/win.txt b/win.txt
  --- a/win.txt
  +++ b/win.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  $ rm -r repo
