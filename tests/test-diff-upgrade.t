  $ "$TESTDIR/hghave" execbit || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "autodiff=$TESTDIR/autodiff.py" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=1" >> $HGRCPATH

  $ hg init repo
  $ cd repo



make a combination of new, changed and deleted file

  $ echo regular > regular
  $ echo rmregular > rmregular
  $ python -c "file('bintoregular', 'wb').write('\0')"
  $ touch rmempty
  $ echo exec > exec
  $ chmod +x exec
  $ echo rmexec > rmexec
  $ chmod +x rmexec
  $ echo setexec > setexec
  $ echo unsetexec > unsetexec
  $ chmod +x unsetexec
  $ echo binary > binary
  $ python -c "file('rmbinary', 'wb').write('\0')"
  $ hg ci -Am addfiles
  adding binary
  adding bintoregular
  adding exec
  adding regular
  adding rmbinary
  adding rmempty
  adding rmexec
  adding rmregular
  adding setexec
  adding unsetexec
  $ echo regular >> regular
  $ echo newregular >> newregular
  $ rm rmempty
  $ touch newempty
  $ rm rmregular
  $ echo exec >> exec
  $ echo newexec > newexec
  $ echo bintoregular > bintoregular
  $ chmod +x newexec
  $ rm rmexec
  $ chmod +x setexec
  $ chmod -x unsetexec
  $ python -c "file('binary', 'wb').write('\0\0')"
  $ python -c "file('newbinary', 'wb').write('\0')"
  $ rm rmbinary
  $ hg addremove -s 0
  adding newbinary
  adding newempty
  adding newexec
  adding newregular
  removing rmbinary
  removing rmempty
  removing rmexec
  removing rmregular

git=no: regular diff for all files

  $ hg autodiff --git=no
  diff -r a66d19b9302d binary
  Binary file binary has changed
  diff -r a66d19b9302d bintoregular
  Binary file bintoregular has changed
  diff -r a66d19b9302d exec
  --- a/exec
  +++ b/exec
  @@ -1,1 +1,2 @@
   exec
  +exec
  diff -r a66d19b9302d newbinary
  Binary file newbinary has changed
  diff -r a66d19b9302d newexec
  --- /dev/null
  +++ b/newexec
  @@ -0,0 +1,1 @@
  +newexec
  diff -r a66d19b9302d newregular
  --- /dev/null
  +++ b/newregular
  @@ -0,0 +1,1 @@
  +newregular
  diff -r a66d19b9302d regular
  --- a/regular
  +++ b/regular
  @@ -1,1 +1,2 @@
   regular
  +regular
  diff -r a66d19b9302d rmbinary
  Binary file rmbinary has changed
  diff -r a66d19b9302d rmexec
  --- a/rmexec
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -rmexec
  diff -r a66d19b9302d rmregular
  --- a/rmregular
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -rmregular

git=yes: git diff for single regular file

  $ hg autodiff --git=yes regular
  diff --git a/regular b/regular
  --- a/regular
  +++ b/regular
  @@ -1,1 +1,2 @@
   regular
  +regular

git=auto: regular diff for regular files and non-binary removals

  $ hg autodiff --git=auto regular newregular rmregular rmexec
  diff -r a66d19b9302d newregular
  --- /dev/null
  +++ b/newregular
  @@ -0,0 +1,1 @@
  +newregular
  diff -r a66d19b9302d regular
  --- a/regular
  +++ b/regular
  @@ -1,1 +1,2 @@
   regular
  +regular
  diff -r a66d19b9302d rmexec
  --- a/rmexec
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -rmexec
  diff -r a66d19b9302d rmregular
  --- a/rmregular
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -rmregular

  $ for f in exec newexec setexec unsetexec binary newbinary newempty rmempty rmbinary bintoregular; do
  >     echo
  >     echo '% git=auto: git diff for' $f
  >     hg autodiff --git=auto $f
  > done
  
  % git=auto: git diff for exec
  diff -r a66d19b9302d exec
  --- a/exec
  +++ b/exec
  @@ -1,1 +1,2 @@
   exec
  +exec
  
  % git=auto: git diff for newexec
  diff --git a/newexec b/newexec
  new file mode 100755
  --- /dev/null
  +++ b/newexec
  @@ -0,0 +1,1 @@
  +newexec
  
  % git=auto: git diff for setexec
  diff --git a/setexec b/setexec
  old mode 100644
  new mode 100755
  
  % git=auto: git diff for unsetexec
  diff --git a/unsetexec b/unsetexec
  old mode 100755
  new mode 100644
  
  % git=auto: git diff for binary
  diff --git a/binary b/binary
  index a9128c283485202893f5af379dd9beccb6e79486..09f370e38f498a462e1ca0faa724559b6630c04f
  GIT binary patch
  literal 2
  Jc${Nk0000200961
  
  
  % git=auto: git diff for newbinary
  diff --git a/newbinary b/newbinary
  new file mode 100644
  index e69de29bb2d1d6434b8b29ae775ad8c2e48c5391..f76dd238ade08917e6712764a16a22005a50573d
  GIT binary patch
  literal 1
  Ic${MZ000310RR91
  
  
  % git=auto: git diff for newempty
  diff --git a/newempty b/newempty
  new file mode 100644
  
  % git=auto: git diff for rmempty
  diff --git a/rmempty b/rmempty
  deleted file mode 100644
  
  % git=auto: git diff for rmbinary
  diff --git a/rmbinary b/rmbinary
  deleted file mode 100644
  index f76dd238ade08917e6712764a16a22005a50573d..e69de29bb2d1d6434b8b29ae775ad8c2e48c5391
  GIT binary patch
  literal 0
  Hc$@<O00001
  
  
  % git=auto: git diff for bintoregular
  diff --git a/bintoregular b/bintoregular
  index f76dd238ade08917e6712764a16a22005a50573d..9c42f2b6427d8bf034b7bc23986152dc01bfd3ab
  GIT binary patch
  literal 13
  Uc$`bh%qz(+N=+}#Ni5<5043uE82|tP
  


git=warn: regular diff with data loss warnings

  $ hg autodiff --git=warn
  diff -r a66d19b9302d binary
  Binary file binary has changed
  diff -r a66d19b9302d bintoregular
  Binary file bintoregular has changed
  diff -r a66d19b9302d exec
  --- a/exec
  +++ b/exec
  @@ -1,1 +1,2 @@
   exec
  +exec
  diff -r a66d19b9302d newbinary
  Binary file newbinary has changed
  diff -r a66d19b9302d newexec
  --- /dev/null
  +++ b/newexec
  @@ -0,0 +1,1 @@
  +newexec
  diff -r a66d19b9302d newregular
  --- /dev/null
  +++ b/newregular
  @@ -0,0 +1,1 @@
  +newregular
  diff -r a66d19b9302d regular
  --- a/regular
  +++ b/regular
  @@ -1,1 +1,2 @@
   regular
  +regular
  diff -r a66d19b9302d rmbinary
  Binary file rmbinary has changed
  diff -r a66d19b9302d rmexec
  --- a/rmexec
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -rmexec
  diff -r a66d19b9302d rmregular
  --- a/rmregular
  +++ /dev/null
  @@ -1,1 +0,0 @@
  -rmregular
  data lost for: binary
  data lost for: bintoregular
  data lost for: newbinary
  data lost for: newempty
  data lost for: newexec
  data lost for: rmbinary
  data lost for: rmempty
  data lost for: setexec
  data lost for: unsetexec

git=abort: fail on execute bit change

  $ hg autodiff --git=abort regular setexec
  abort: losing data for setexec
  [255]

git=abort: succeed on regular file

  $ hg autodiff --git=abort regular
  diff -r a66d19b9302d regular
  --- a/regular
  +++ b/regular
  @@ -1,1 +1,2 @@
   regular
  +regular

  $ cd ..

