
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80

  $ fixpath()
  > {
  >     tr '\\' /
  > }
  $ cat > $HGRCPATH <<EOF
  > [extensions]
  > convert = 
  > graphlog =
  > EOF

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/move.svndump"
  $ svnpath=`pwd | fixpath`

SVN wants all paths to start with a slash. Unfortunately,
Windows ones don't. Handle that.

  $ expr "$svnpath" : "\/" > /dev/null
  > if [ $? -ne 0 ]; then
  >   svnpath="/$svnpath"
  > fi
  > svnurl="file://$svnpath/svn-repo"

Convert trunk and branches

  $ hg convert --datesort "$svnurl"/subproject A-hg
  initializing destination A-hg repository
  scanning source...
  sorting...
  converting...
  13 createtrunk
  12 moved1
  11 moved1
  10 moved2
  9 changeb and rm d2
  8 changeb and rm d2
  7 moved1again
  6 moved1again
  5 copyfilefrompast
  4 copydirfrompast
  3 add d3
  2 copy dir and remove subdir
  1 add d4old
  0 rename d4old into d4new

  $ cd A-hg
  $ hg glog --template '{rev} {desc|firstline} files: {files}\n'
  o  13 rename d4old into d4new files: d4new/g d4old/g
  |
  o  12 add d4old files: d4old/g
  |
  o  11 copy dir and remove subdir files: d3/d31/e d4/d31/e d4/f
  |
  o  10 add d3 files: d3/d31/e d3/f
  |
  o  9 copydirfrompast files: d2/d
  |
  o  8 copyfilefrompast files: d
  |
  o  7 moved1again files: d1/b d1/c
  |
  | o  6 moved1again files:
  | |
  o |  5 changeb and rm d2 files: d1/b d2/d
  | |
  | o  4 changeb and rm d2 files: b
  | |
  o |  3 moved2 files: d2/d
  | |
  o |  2 moved1 files: d1/b d1/c
  | |
  | o  1 moved1 files: b c
  |
  o  0 createtrunk files:
  

Check move copy records

  $ hg st --rev 12:13 --copies
  A d4new/g
    d4old/g
  R d4old/g

Check branches

  $ hg branches
  default                       13:.*
  d1                             6:.*
  $ cd ..

  $ mkdir test-replace
  $ cd test-replace
  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/replace.svndump"

Convert files being replaced by directories

  $ hg convert svn-repo hg-repo
  initializing destination hg-repo repository
  scanning source...
  sorting...
  converting...
  3 initial
  2 clobber symlink
  1 clobber1
  0 clobber2

  $ cd hg-repo

Manifest before

  $ hg -v manifest -r 1
  644   a
  644   d/b
  644 @ dlink
  644 @ dlink2
  644   dlink3

Manifest after clobber1

  $ hg -v manifest -r 2
  644   a/b
  644   d/b
  644   dlink/b
  644 @ dlink2
  644   dlink3

Manifest after clobber2

  $ hg -v manifest -r 3
  644   a/b
  644   d/b
  644   dlink/b
  644 @ dlink2
  644 @ dlink3

Try updating

  $ hg up -qC default
  $ cd ..

Test convert progress bar'

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > progress = 
  > [progress]
  > assume-tty = 1
  > delay = 0
  > refresh = 0
  > EOF
  $ cat > filtercr.py <<EOF
  > import sys, re
  > for line in sys.stdin:
  >     line = re.sub(r'\r+[^\n]', lambda m: '\n' + m.group()[-1:], line)
  >     sys.stdout.write(line)
  > EOF

  $ hg convert svn-repo hg-progress 2>&1 | python filtercr.py
  
  scanning [ <=>                                                              ] 1
  scanning [  <=>                                                             ] 2
  scanning [   <=>                                                            ] 3
  scanning [    <=>                                                           ] 4
                                                                                  
  converting [                                                              ] 0/4
  getting files [==========>                                                ] 1/5
  getting files [======================>                                    ] 2/5
  getting files [==================================>                        ] 3/5
  getting files [==============================================>            ] 4/5
  getting files [==========================================================>] 5/5
                                                                                  
  converting [==============>                                               ] 1/4
  scanning paths [                                                          ] 0/1
                                                                                  
  getting files [==========================================================>] 1/1
                                                                                  
  converting [==============================>                               ] 2/4
  scanning paths [                                                          ] 0/2
  scanning paths [============================>                             ] 1/2
                                                                                  
  getting files [=============>                                             ] 1/4
  getting files [============================>                              ] 2/4
  getting files [===========================================>               ] 3/4
  getting files [==========================================================>] 4/4
                                                                                  
  converting [=============================================>                ] 3/4
  scanning paths [                                                          ] 0/1
                                                                                  
  getting files [==========================================================>] 1/1
                                                                                  
  initializing destination hg-progress repository
  scanning source...
  sorting...
  converting...
  3 initial
  2 clobber symlink
  1 clobber1
  0 clobber2
