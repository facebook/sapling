Test EOL update

  $ cat > $HGRCPATH <<EOF
  > [diff]
  > git = 1
  > EOF

  $ seteol () {
  >     if [ $1 = "LF" ]; then
  >         EOL='\n'
  >     else
  >         EOL='\r\n'
  >     fi
  > }

  $ makerepo () {
  >     echo
  >     echo "# ==== setup repository ===="
  >     echo '% hg init'
  >     hg init repo
  >     cd repo
  > 
  >     cat > .hgeol <<EOF
  > [patterns]
  > **.txt = LF
  > EOF
  > 
  >     printf "first\nsecond\nthird\n" > a.txt
  >     hg commit --addremove -m 'LF commit'
  > 
  >     cat > .hgeol <<EOF
  > [patterns]
  > **.txt = CRLF
  > EOF
  > 
  >     printf "first\r\nsecond\r\nthird\r\n" > a.txt
  >     hg commit -m 'CRLF commit'
  > 
  >     cd ..
  > }

  $ dotest () {
  >     seteol $1
  > 
  >     echo
  >     echo "% hg clone repo repo-$1"
  >     hg clone --noupdate repo repo-$1
  >     cd repo-$1
  > 
  >     cat > .hg/hgrc <<EOF
  > [extensions]
  > eol =
  > EOF
  > 
  >     hg update
  > 
  >     echo '% a.txt (before)'
  >     cat a.txt
  > 
  >     printf "first${EOL}third${EOL}" > a.txt
  > 
  >     echo '% a.txt (after)'
  >     cat a.txt
  >     echo '% hg diff'
  >     hg diff
  > 
  >     echo '% hg update 0'
  >     hg update 0
  > 
  >     echo '% a.txt'
  >     cat a.txt
  >     echo '% hg diff'
  >     hg diff
  > 
  > 
  >     cd ..
  >     rm -r repo-$1
  > }

  $ makerepo
  
  # ==== setup repository ====
  % hg init
  adding .hgeol
  adding a.txt
  $ dotest LF
  
  % hg clone repo repo-LF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % a.txt (before)
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % a.txt (after)
  first
  third
  % hg diff
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  % hg update 0
  merging a.txt
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  % a.txt
  first
  third
  % hg diff
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  $ dotest CRLF
  
  % hg clone repo repo-CRLF
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % a.txt (before)
  first\r (esc)
  second\r (esc)
  third\r (esc)
  % a.txt (after)
  first\r (esc)
  third\r (esc)
  % hg diff
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,2 @@
   first\r (esc)
  -second\r (esc)
   third\r (esc)
  % hg update 0
  merging a.txt
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  % a.txt
  first
  third
  % hg diff
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,2 @@
   first
  -second
   third
  $ rm -r repo
