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
  matcher: <alwaysmatcher>
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
