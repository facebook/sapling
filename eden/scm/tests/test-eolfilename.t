#chg-compatible
#debugruntest-compatible

#require no-fsmonitor

#require eol-in-paths

  $ eagerepo
  $ setconfig workingcopy.ruststatus=false

https://bz.mercurial-scm.org/352

test issue352

  $ newclientrepo
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
  hell
  o: Failed to validate "hell\no". Invalid byte: 10.
  [1]
  $ hg ci -A -m m
  abort: hell
  o: Failed to validate "hell\no". Invalid byte: 10.
  [255]
  $ echo foo > "$A"
  $ hg debugwalk
  matcher: <alwaysmatcher>
  hell
  o: Failed to validate "hell\no". Invalid byte: 10.
  f  he\r (no-eol) (esc)
  llo  he\r (no-eol) (esc)
  llo

  $ echo bla > quickfox
  $ hg add quickfox
  hell
  o: Failed to validate "hell\no". Invalid byte: 10.
  [1]
  $ hg ci -m 2
  abort: hell
  o: Failed to validate "hell\no". Invalid byte: 10.
  [255]
  $ A=`printf 'quick\rfox'`
  $ hg cp quickfox "$A"
  hell
  o: Failed to validate "hell\no". Invalid byte: 10.
  abort: '\n' and '\r' disallowed in filenames: 'quick\rfox'
  [255]
  $ hg mv quickfox "$A"
  hell
  o: Failed to validate "hell\no". Invalid byte: 10.
  abort: '\n' and '\r' disallowed in filenames: 'quick\rfox'
  [255]

https://bz.mercurial-scm.org/2036

  $ cd ..

test issue2039

  $ newclientrepo
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > color =
  > [color]
  > mode = ansi
  > EOF
  $ A=`printf 'foo\nbar'`
  $ B=`printf 'foo\nbar.baz'`
  $ touch "$A"
  $ touch "$B"

  $ hg status --color=always
  abort: path error at 'foo
  bar.baz': Failed to validate "foo\nbar.baz". Invalid byte: 10.
  [255]

  $ cd ..
