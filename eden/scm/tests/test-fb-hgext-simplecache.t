#chg-compatible

  $ setconfig extensions.treemanifest=!
#require no-osx

Setup the extension

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > simplecache=
  > [simplecache]
  > showdebug=true
  > cachedir=$TESTTMP/hgsimplecache
  > EOF

Initialize the repo

  $ hg init repo
  $ cd repo

  $ echo x >> x
  $ hg commit -Am x
  adding x
  $ hg mv x y
  $ echo x >> y
  $ hg st -C
  A y
    x
  R x
  $ hg commit -Am xx
  $ hg book foo

Test that output remains the same with multiple invocations.
  $ hg --debug export
  exporting patch:
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID a5d935fe38ada2b984c29e4e02bffd7f19bf818d
  # Parent  b292c1e3311fd0f13ae83b409caae4a6d1fb348c
  xx
  
  no value found for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 from local
  falling back for value buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2
  set value for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 to local
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -x
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d y
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +x
  +x
  $ hg --debug export
  exporting patch:
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID a5d935fe38ada2b984c29e4e02bffd7f19bf818d
  # Parent  b292c1e3311fd0f13ae83b409caae4a6d1fb348c
  xx
  
  got value for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 from local
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -x
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d y
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +x
  +x
  $ hg --debug log -vpC -r .
  got value for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 from local
  got value for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 from local
  changeset:   1:a5d935fe38ada2b984c29e4e02bffd7f19bf818d
  bookmark:    foo
  phase:       draft
  parent:      0:b292c1e3311fd0f13ae83b409caae4a6d1fb348c
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    87b34bbb5b48d4106fd2da95fbe8beb73c1850d2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      y
  files-:      x
  copies:      y (x)
  extra:       branch=default
  description:
  xx
  
  
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -x
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d y
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +x
  +x
  
  $ hg log -vp -r .
  changeset:   1:a5d935fe38ad
  bookmark:    foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       x y
  description:
  xx
  
  
  diff -r b292c1e3311f -r a5d935fe38ad x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -x
  diff -r b292c1e3311f -r a5d935fe38ad y
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +x
  +x
  

Test that disabling the cache entirely doesn't break things
  $ hg --debug --config simplecache.caches= export
  exporting patch:
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID a5d935fe38ada2b984c29e4e02bffd7f19bf818d
  # Parent  b292c1e3311fd0f13ae83b409caae4a6d1fb348c
  xx
  
  falling back for value buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -x
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d y
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +x
  +x

Test that corrupt caches are gracefully ignored, and updated
# Use a long value so we trigger checksum validation
  $ printf "12345678901234567890123456789012345678901234567890" > "$TESTTMP/hgsimplecache/buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2"
  $ hg --debug log -vpC -r .
  got value for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 from local
  error getting or deserializing key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2: invalid hash from simplecache for key 'buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2'
  no value found for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 from local
  falling back for value buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2
  set value for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 to local
  got value for key buildstatus:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:b292c1e3311fd0f13ae83b409caae4a6d1fb348c:v2 from local
  changeset:   1:a5d935fe38ada2b984c29e4e02bffd7f19bf818d
  bookmark:    foo
  phase:       draft
  parent:      0:b292c1e3311fd0f13ae83b409caae4a6d1fb348c
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    87b34bbb5b48d4106fd2da95fbe8beb73c1850d2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      y
  files-:      x
  copies:      y (x)
  extra:       branch=default
  description:
  xx
  
  
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d x
  --- a/x	Thu Jan 01 00:00:00 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -x
  diff -r b292c1e3311fd0f13ae83b409caae4a6d1fb348c -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d y
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/y	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,2 @@
  +x
  +x
  

Test strange (unicode) filenames
  $ printf "delete cca.hg.buildstatus:f3a143469693894d291b7388ea8392a07492751f:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:v2\r\n" | nc localhost 11101
  * (glob)
  $ echo 'x' > Å
  $ hg add Å
  $ hg commit -m 'unicode test'
  $ hg export --debug
  exporting patch:
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID f3a143469693894d291b7388ea8392a07492751f
  # Parent  a5d935fe38ada2b984c29e4e02bffd7f19bf818d
  unicode test
  
  no value found for key buildstatus:f3a143469693894d291b7388ea8392a07492751f:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:v2 from local
  falling back for value buildstatus:f3a143469693894d291b7388ea8392a07492751f:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:v2
  set value for key buildstatus:f3a143469693894d291b7388ea8392a07492751f:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:v2 to local
  diff -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d -r f3a143469693894d291b7388ea8392a07492751f \xc3\x85 (esc)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/\xc3\x85	Thu Jan 01 00:00:00 1970 +0000 (esc)
  @@ -0,0 +1,1 @@
  +x
  $ hg --debug log -vpC -r .
  got value for key buildstatus:f3a143469693894d291b7388ea8392a07492751f:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:v2 from local
  got value for key buildstatus:f3a143469693894d291b7388ea8392a07492751f:a5d935fe38ada2b984c29e4e02bffd7f19bf818d:v2 from local
  changeset:   2:f3a143469693894d291b7388ea8392a07492751f
  bookmark:    foo
  phase:       draft
  parent:      1:a5d935fe38ada2b984c29e4e02bffd7f19bf818d
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    0640a75d79d252a6348f1e2316af42e7960b4a90
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      \xc3\x85 (esc)
  extra:       branch=default
  description:
  unicode test
  
  
  diff -r a5d935fe38ada2b984c29e4e02bffd7f19bf818d -r f3a143469693894d291b7388ea8392a07492751f \xc3\x85 (esc)
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/\xc3\x85	Thu Jan 01 00:00:00 1970 +0000 (esc)
  @@ -0,0 +1,1 @@
  +x
  
Test local cache eviction
  $ echo 'x' >> y && hg commit -qm "1" && hg export --debug > /dev/null
  $ echo 'x' >> y && hg commit -qm "2" && hg export --debug > /dev/null
  $ echo 'x' >> y && hg commit -qm "3" && hg export --debug > /dev/null
  $ echo 'x' >> y && hg commit -qm "4" && hg export --debug > /dev/null
  $ echo 'x' >> y && hg commit -qm "5" && hg export --debug > /dev/null
  $ echo 'x' >> y && hg commit -qm "6" && hg export --debug > /dev/null
  $ ls $TESTTMP/hgsimplecache | grep -c buildstatus
  8
  $ echo 'x' >> y && hg commit -qm "7"
  $ hg --debug --config simplecache.maxcachesize=2 --config simplecache.evictionpercent=50 export > /dev/null
  $ ls $TESTTMP/hgsimplecache | grep -c buildstatus
  5
