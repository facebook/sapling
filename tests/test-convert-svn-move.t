
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert =
  > graphlog =
  > EOF

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/move.svndump"
  $ SVNREPOPATH=`pwd`/svn-repo
#if windows
  $ SVNREPOURL=file:///`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#else
  $ SVNREPOURL=file://`python -c "import urllib, sys; sys.stdout.write(urllib.quote(sys.argv[1]))" "$SVNREPOPATH"`
#endif

Convert trunk and branches

  $ hg convert --datesort "$SVNREPOURL"/subproject A-hg
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
  default                       13:* (glob)
  d1                             6:* (glob)
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
  6 initial
  5 clobber symlink
  4 clobber1
  3 clobber2
  2 adddb
  1 clobberdir
  0 branch

  $ cd hg-repo

Manifest before

  $ hg -v manifest -r 1
  644   a
  644   d/b
  644   d2/a
  644 @ dlink
  644 @ dlink2
  644   dlink3

Manifest after clobber1

  $ hg -v manifest -r 2
  644   a/b
  644   d/b
  644   d2/a
  644   dlink/b
  644 @ dlink2
  644   dlink3

Manifest after clobber2

  $ hg -v manifest -r 3
  644   a/b
  644   d/b
  644   d2/a
  644   dlink/b
  644 @ dlink2
  644 @ dlink3

Manifest after clobberdir

  $ hg -v manifest -r 6
  644   a/b
  644   d/b
  644   d2/a
  644   d2/c
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
  > changedelay = 0
  > format = topic bar number
  > refresh = 0
  > width = 60
  > EOF

  $ hg convert svn-repo hg-progress
  \r (no-eol) (esc)
  scanning [ <=>                                          ] 1\r (no-eol) (esc)
  scanning [  <=>                                         ] 2\r (no-eol) (esc)
  scanning [   <=>                                        ] 3\r (no-eol) (esc)
  scanning [    <=>                                       ] 4\r (no-eol) (esc)
  scanning [     <=>                                      ] 5\r (no-eol) (esc)
  scanning [      <=>                                     ] 6\r (no-eol) (esc)
  scanning [       <=>                                    ] 7\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  converting [                                          ] 0/7\r (no-eol) (esc)
  getting files [=====>                                 ] 1/6\r (no-eol) (esc)
  getting files [============>                          ] 2/6\r (no-eol) (esc)
  getting files [==================>                    ] 3/6\r (no-eol) (esc)
  getting files [=========================>             ] 4/6\r (no-eol) (esc)
  getting files [===============================>       ] 5/6\r (no-eol) (esc)
  getting files [======================================>] 6/6\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  converting [=====>                                    ] 1/7\r (no-eol) (esc)
  scanning paths [                                      ] 0/1\r (no-eol) (esc)
  getting files [======================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  converting [===========>                              ] 2/7\r (no-eol) (esc)
  scanning paths [                                      ] 0/2\r (no-eol) (esc)
  scanning paths [==================>                   ] 1/2\r (no-eol) (esc)
  getting files [========>                              ] 1/4\r (no-eol) (esc)
  getting files [==================>                    ] 2/4\r (no-eol) (esc)
  getting files [============================>          ] 3/4\r (no-eol) (esc)
  getting files [======================================>] 4/4\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  converting [=================>                        ] 3/7\r (no-eol) (esc)
  scanning paths [                                      ] 0/1\r (no-eol) (esc)
  getting files [======================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  converting [=======================>                  ] 4/7\r (no-eol) (esc)
  scanning paths [                                      ] 0/1\r (no-eol) (esc)
  getting files [======================================>] 1/1\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  converting [=============================>            ] 5/7\r (no-eol) (esc)
  scanning paths [                                      ] 0/1\r (no-eol) (esc)
  getting files [===>                                   ] 1/8\r (no-eol) (esc)
  getting files [========>                              ] 2/8\r (no-eol) (esc)
  getting files [=============>                         ] 3/8\r (no-eol) (esc)
  getting files [==================>                    ] 4/8\r (no-eol) (esc)
  getting files [=======================>               ] 5/8\r (no-eol) (esc)
  getting files [============================>          ] 6/8\r (no-eol) (esc)
  getting files [=================================>     ] 7/8\r (no-eol) (esc)
  getting files [======================================>] 8/8\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  \r (no-eol) (esc)
  converting [===================================>      ] 6/7\r (no-eol) (esc)
  scanning paths [                                      ] 0/3\r (no-eol) (esc)
  scanning paths [===========>                          ] 1/3\r (no-eol) (esc)
  scanning paths [========================>             ] 2/3\r (no-eol) (esc)
  getting files [===>                                   ] 1/8\r (no-eol) (esc)
  getting files [========>                              ] 2/8\r (no-eol) (esc)
  getting files [=============>                         ] 3/8\r (no-eol) (esc)
  getting files [==================>                    ] 4/8\r (no-eol) (esc)
  getting files [=======================>               ] 5/8\r (no-eol) (esc)
  getting files [============================>          ] 6/8\r (no-eol) (esc)
  getting files [=================================>     ] 7/8\r (no-eol) (esc)
  getting files [======================================>] 8/8\r (no-eol) (esc)
                                                              \r (no-eol) (esc)
  initializing destination hg-progress repository
  scanning source...
  sorting...
  converting...
  6 initial
  5 clobber symlink
  4 clobber1
  3 clobber2
  2 adddb
  1 clobberdir
  0 branch

  $ cd ..
