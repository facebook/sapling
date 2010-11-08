http://mercurial.selenic.com/bts/issue352

  $ "$TESTDIR/hghave" eol-in-paths || exit 80

test issue352

  $ hg init foo
  $ cd foo
  $ A=`printf 'he\rllo'`
  $ echo foo > "$A"
  $ hg add
  adding he\rllo (esc)
  abort: '\n' and '\r' disallowed in filenames: 'he\rllo'
  [255]
  $ hg ci -A -m m
  adding he\rllo (esc)
  abort: '\n' and '\r' disallowed in filenames: 'he\rllo'
  [255]
  $ rm "$A"
  $ echo foo > "hell
  > o"
  $ hg add
  adding hell
  o
  abort: '\n' and '\r' disallowed in filenames: 'hell\no'
  [255]
  $ hg ci -A -m m
  adding hell
  o
  abort: '\n' and '\r' disallowed in filenames: 'hell\no'
  [255]
  $ echo foo > "$A"
  $ hg debugwalk
  f  he\rllo  he\rllo (esc)
  f  hell
  o  hell
  o

http://mercurial.selenic.com/bts/issue2036

  $ cd ..

test issue2039

  $ hg init bar
  $ cd bar
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "color=" >> $HGRCPATH
  $ A=`printf 'foo\nbar'`
  $ B=`printf 'foo\nbar.baz'`
  $ touch "$A"
  $ touch "$B"
  $ hg status --color=always
  \x1b[0;35;1;4m? foo\x1b[0m (esc)
  \x1b[0;35;1;4mbar\x1b[0m (esc)
  \x1b[0;35;1;4m? foo\x1b[0m (esc)
  \x1b[0;35;1;4mbar.baz\x1b[0m (esc)
