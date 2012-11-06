http://mercurial.selenic.com/bts/issue352

  $ "$TESTDIR/hghave" eol-in-paths || exit 80

test issue352

  $ hg init foo
  $ cd foo
  $ A=`printf 'he\rllo'`
  $ echo foo > "$A"
  $ hg add
  adding he\r (no-eol) (esc)
  llo
  abort: '\n' and '\r' disallowed in filenames: 'he\rllo'
  [255]
  $ hg ci -A -m m
  adding he\r (no-eol) (esc)
  llo
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
  f  he\r (no-eol) (esc)
  llo  he\r (no-eol) (esc)
  llo
  f  hell
  o  hell
  o

  $ echo bla > quickfox
  $ hg add quickfox
  $ hg ci -m 2
  $ A=`printf 'quick\rfox'`
  $ hg cp quickfox "$A"
  abort: '\n' and '\r' disallowed in filenames: 'quick\rfox'
  [255]
  $ hg mv quickfox "$A"
  abort: '\n' and '\r' disallowed in filenames: 'quick\rfox'
  [255]

http://mercurial.selenic.com/bts/issue2036

  $ cd ..

test issue2039

  $ hg init bar
  $ cd bar
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "color=" >> $HGRCPATH
  $ echo "[color]" >> $HGRCPATH
  $ echo "mode = ansi" >> $HGRCPATH
  $ A=`printf 'foo\nbar'`
  $ B=`printf 'foo\nbar.baz'`
  $ touch "$A"
  $ touch "$B"
  $ hg status --color=always
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mfoo\x1b[0m (esc)
  \x1b[0;35;1;4mbar\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mfoo\x1b[0m (esc)
  \x1b[0;35;1;4mbar.baz\x1b[0m (esc)

  $ cd ..
