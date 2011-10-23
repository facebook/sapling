
  $ "$TESTDIR/hghave" svn13 no-outer-repo || exit 80

  $ fixpath()
  > {
  >     tr '\\' /
  > }
  $ svnupanddisplay()
  > {
  >     (
  >        cd $1;
  >        svn up;
  >        svn st -v | fixpath | sed 's/  */ /g'
  >        limit=''
  >        if [ $2 -gt 0 ]; then
  >            limit="--limit=$2"
  >        fi
  >        svn log --xml -v $limit \
  >            | fixpath \
  >            | sed 's,<date>.*,<date/>,' \
  >            | grep -v 'kind="'
  >     )
  > }

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert = 
  > graphlog =
  > EOF

  $ hg init a

Add

  $ echo a > a/a
  $ mkdir -p a/d1/d2
  $ echo b > a/d1/d2/b
  $ ln -s a/missing a/link
  $ hg --cwd a ci -d '0 0' -A -m 'add a file'
  adding a
  adding d1/d2/b
  adding link

Modify

  $ "$TESTDIR/svn-safe-append.py" a a/a
  $ hg --cwd a ci -d '1 0' -m 'modify a file'
  $ hg --cwd a tip -q
  1:8231f652da37

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn repository 'a-hg'
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  1 add a file
  0 modify a file
  $ svnupanddisplay a-hg-wc 2
  At revision 2.
   2 2 test .
   2 2 test a
   2 1 test d1
   2 1 test d1/d2
   2 1 test d1/d2/b
   2 1 test link
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="2">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="M">/a</path>
  </paths>
  <msg>modify a file</msg>
  </logentry>
  <logentry
     revision="1">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="A">/a</path>
  <path
     action="A">/d1</path>
  <path
     action="A">/d1/d2</path>
  <path
     action="A">/d1/d2/b</path>
  <path
     action="A">/link</path>
  </paths>
  <msg>add a file</msg>
  </logentry>
  </log>
  $ ls a a-hg-wc
  a:
  a
  d1
  link
  
  a-hg-wc:
  a
  d1
  link
  $ cmp a/a a-hg-wc/a

Rename

  $ hg --cwd a mv a b
  $ hg --cwd a mv link newlink

  $ hg --cwd a ci -d '2 0' -m 'rename a file'
  $ hg --cwd a tip -q
  2:a67e26ccec09

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  0 rename a file
  $ svnupanddisplay a-hg-wc 1
  At revision 3.
   3 3 test .
   3 3 test b
   3 1 test d1
   3 1 test d1/d2
   3 1 test d1/d2/b
   3 3 test newlink
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="3">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="D">/a</path>
  <path
     copyfrom-path="/a"
     copyfrom-rev="2"
     action="A">/b</path>
  <path
     copyfrom-path="/link"
     copyfrom-rev="2"
     action="A">/newlink</path>
  <path
     action="D">/link</path>
  </paths>
  <msg>rename a file</msg>
  </logentry>
  </log>
  $ ls a a-hg-wc
  a:
  b
  d1
  newlink
  
  a-hg-wc:
  b
  d1
  newlink

Copy

  $ hg --cwd a cp b c

  $ hg --cwd a ci -d '3 0' -m 'copy a file'
  $ hg --cwd a tip -q
  3:0cf087b9ab02

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  0 copy a file
  $ svnupanddisplay a-hg-wc 1
  At revision 4.
   4 4 test .
   4 3 test b
   4 4 test c
   4 1 test d1
   4 1 test d1/d2
   4 1 test d1/d2/b
   4 3 test newlink
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="4">
  <author>test</author>
  <date/>
  <paths>
  <path
     copyfrom-path="/b"
     copyfrom-rev="3"
     action="A">/c</path>
  </paths>
  <msg>copy a file</msg>
  </logentry>
  </log>
  $ ls a a-hg-wc
  a:
  b
  c
  d1
  newlink
  
  a-hg-wc:
  b
  c
  d1
  newlink

  $ hg --cwd a rm b

Remove

  $ hg --cwd a ci -d '4 0' -m 'remove a file'
  $ hg --cwd a tip -q
  4:07b2e34a5b17

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  0 remove a file
  $ svnupanddisplay a-hg-wc 1
  At revision 5.
   5 5 test .
   5 4 test c
   5 1 test d1
   5 1 test d1/d2
   5 1 test d1/d2/b
   5 3 test newlink
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="5">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="D">/b</path>
  </paths>
  <msg>remove a file</msg>
  </logentry>
  </log>
  $ ls a a-hg-wc
  a:
  c
  d1
  newlink
  
  a-hg-wc:
  c
  d1
  newlink

Exectutable

  $ chmod +x a/c
  $ hg --cwd a ci -d '5 0' -m 'make a file executable'
  $ hg --cwd a tip -q
  5:31093672760b

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  0 make a file executable
  $ svnupanddisplay a-hg-wc 1
  At revision 6.
   6 6 test .
   6 6 test c
   6 1 test d1
   6 1 test d1/d2
   6 1 test d1/d2/b
   6 3 test newlink
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="6">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="M">/c</path>
  </paths>
  <msg>make a file executable</msg>
  </logentry>
  </log>
  $ test -x a-hg-wc/c

Executable in new directory

  $ rm -rf a a-hg a-hg-wc
  $ hg init a

  $ mkdir a/d1
  $ echo a > a/d1/a
  $ chmod +x a/d1/a
  $ hg --cwd a ci -d '0 0' -A -m 'add executable file in new directory'
  adding d1/a

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn repository 'a-hg'
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  0 add executable file in new directory
  $ svnupanddisplay a-hg-wc 1
  At revision 1.
   1 1 test .
   1 1 test d1
   1 1 test d1/a
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="1">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="A">/d1</path>
  <path
     action="A">/d1/a</path>
  </paths>
  <msg>add executable file in new directory</msg>
  </logentry>
  </log>
  $ test -x a-hg-wc/d1/a

Copy to new directory

  $ mkdir a/d2
  $ hg --cwd a cp d1/a d2/a
  $ hg --cwd a ci -d '1 0' -A -m 'copy file to new directory'

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  0 copy file to new directory
  $ svnupanddisplay a-hg-wc 1
  At revision 2.
   2 2 test .
   2 1 test d1
   2 1 test d1/a
   2 2 test d2
   2 2 test d2/a
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="2">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="A">/d2</path>
  <path
     copyfrom-path="/d1/a"
     copyfrom-rev="1"
     action="A">/d2/a</path>
  </paths>
  <msg>copy file to new directory</msg>
  </logentry>
  </log>

Branchy history

  $ hg init b
  $ echo base > b/b
  $ hg --cwd b ci -d '0 0' -Ambase
  adding b

  $ "$TESTDIR/svn-safe-append.py" left-1 b/b
  $ echo left-1 > b/left-1
  $ hg --cwd b ci -d '1 0' -Amleft-1
  adding left-1

  $ "$TESTDIR/svn-safe-append.py" left-2 b/b
  $ echo left-2 > b/left-2
  $ hg --cwd b ci -d '2 0' -Amleft-2
  adding left-2

  $ hg --cwd b up 0
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ "$TESTDIR/svn-safe-append.py" right-1 b/b
  $ echo right-1 > b/right-1
  $ hg --cwd b ci -d '3 0' -Amright-1
  adding right-1
  created new head

  $ "$TESTDIR/svn-safe-append.py" right-2 b/b
  $ echo right-2 > b/right-2
  $ hg --cwd b ci -d '4 0' -Amright-2
  adding right-2

  $ hg --cwd b up -C 2
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg --cwd b merge
  merging b
  warning: conflicts during merge.
  merging b failed!
  2 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg --cwd b revert -r 2 b
  $ hg resolve -m b
  $ hg --cwd b ci -d '5 0' -m 'merge'

Expect 4 changes

  $ hg convert -d svn b
  assuming destination b-hg
  initializing svn repository 'b-hg'
  initializing svn working copy 'b-hg-wc'
  scanning source...
  sorting...
  converting...
  5 base
  4 left-1
  3 left-2
  2 right-1
  1 right-2
  0 merge

  $ svnupanddisplay b-hg-wc 0
  At revision 4.
   4 4 test .
   4 3 test b
   4 2 test left-1
   4 3 test left-2
   4 4 test right-1
   4 4 test right-2
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="4">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="A">/right-1</path>
  <path
     action="A">/right-2</path>
  </paths>
  <msg>merge</msg>
  </logentry>
  <logentry
     revision="3">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="M">/b</path>
  <path
     action="A">/left-2</path>
  </paths>
  <msg>left-2</msg>
  </logentry>
  <logentry
     revision="2">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="M">/b</path>
  <path
     action="A">/left-1</path>
  </paths>
  <msg>left-1</msg>
  </logentry>
  <logentry
     revision="1">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="A">/b</path>
  </paths>
  <msg>base</msg>
  </logentry>
  </log>

Tags are not supported, but must not break conversion

  $ rm -rf a a-hg a-hg-wc
  $ hg init a
  $ echo a > a/a
  $ hg --cwd a ci -d '0 0' -A -m 'Add file a'
  adding a
  $ hg --cwd a tag -d '1 0' -m 'Tagged as v1.0' v1.0

  $ hg convert -d svn a
  assuming destination a-hg
  initializing svn repository 'a-hg'
  initializing svn working copy 'a-hg-wc'
  scanning source...
  sorting...
  converting...
  1 Add file a
  0 Tagged as v1.0
  writing Subversion tags is not yet implemented
  $ svnupanddisplay a-hg-wc 2
  At revision 2.
   2 2 test .
   2 1 test a
   2 2 test .hgtags
  <?xml version="1.0"?>
  <log>
  <logentry
     revision="2">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="A">/.hgtags</path>
  </paths>
  <msg>Tagged as v1.0</msg>
  </logentry>
  <logentry
     revision="1">
  <author>test</author>
  <date/>
  <paths>
  <path
     action="A">/a</path>
  </paths>
  <msg>Add file a</msg>
  </logentry>
  </log>
  $ rm -rf a a-hg a-hg-wc
