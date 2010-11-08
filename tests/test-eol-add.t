Test adding .hgeol

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
  >     printf "first\nsecond\nthird\n" > a.txt
  >     hg commit -d '100 0' --addremove -m 'LF commit'
  >     cd ..
  > }
  $ dotest () {
  >     seteol $1
  >     echo
  >     echo "% hg clone repo repo-$1"
  >     hg clone repo repo-$1
  >     cd repo-$1
  >     cat > .hg/hgrc <<EOF
  > [extensions]
  > eol =
  > [eol]
  > native = LF
  > EOF
  >     cat > .hgeol <<EOF
  > [patterns]
  > **.txt = native
  > [repository]
  > native = $1
  > EOF
  >     echo '% hg add .hgeol'
  >     hg add .hgeol
  >     echo '% hg status'
  >     hg status
  >     echo '% hg commit'
  >     hg commit -d '200 0' -m 'Added .hgeol file'
  >     echo '% hg status'
  >     hg status
  >     echo '% hg tip -p'
  >     hg tip -p
  >     cd ..
  >     rm -r repo-$1
  > }
  $ makerepo
  
  # ==== setup repository ====
  % hg init
  adding a.txt
  $ dotest LF
  
  % hg clone repo repo-LF
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % hg add .hgeol
  % hg status
  A .hgeol
  % hg commit
  % hg status
  % hg tip -p
  changeset:   1:33503edb53b0
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:03:20 1970 +0000
  summary:     Added .hgeol file
  
  diff --git a/.hgeol b/.hgeol
  new file mode 100644
  --- /dev/null
  +++ b/.hgeol
  @@ -0,0 +1,4 @@
  +[patterns]
  +**.txt = native
  +[repository]
  +native = LF
  
  $ dotest CRLF
  
  % hg clone repo repo-CRLF
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  % hg add .hgeol
  % hg status
  M a.txt
  A .hgeol
  % hg commit
  % hg status
  % hg tip -p
  changeset:   1:6e64eaa9eb23
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:03:20 1970 +0000
  summary:     Added .hgeol file
  
  diff --git a/.hgeol b/.hgeol
  new file mode 100644
  --- /dev/null
  +++ b/.hgeol
  @@ -0,0 +1,4 @@
  +[patterns]
  +**.txt = native
  +[repository]
  +native = CRLF
  diff --git a/a.txt b/a.txt
  --- a/a.txt
  +++ b/a.txt
  @@ -1,3 +1,3 @@
  -first
  -second
  -third
  +first\r (esc)
  +second\r (esc)
  +third\r (esc)
  
  $ rm -r repo
