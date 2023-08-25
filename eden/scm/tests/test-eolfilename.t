#debugruntest-compatible

#require no-fsmonitor

#require eol-in-paths

  $ eagerepo

https://bz.mercurial-scm.org/352

test issue352

  $ newclientrepo
  $ A=`printf 'he\rllo'`
  $ echo foo > "$A"
Don't error out if a naughty file happens to be present:
  $ hg add
  skipping invalid path 'he\rllo'
Do error out if the naughty file is explicitly referenced:
  $ hg add "$A"
  abort: Failed to validate "he\rllo". Invalid byte: 13.
  [255]
  $ hg ci -A -m m
  skipping invalid path 'he\rllo'
  skipping invalid path 'he\rllo'
  skipping invalid path 'he\rllo'
  nothing changed
  [1]
  $ rm "$A"
  $ echo foo > "hell
  > o"
  $ hg add
  skipping invalid path 'hell\no'
  $ hg ci -A -m m
  skipping invalid path 'hell\no'
  skipping invalid path 'hell\no'
  skipping invalid path 'hell\no'
  nothing changed
  [1]
  $ echo foo > "$A"
  $ hg debugwalk
  skipping invalid path 'he\rllo'
  skipping invalid path 'hell\no'

  $ echo bla > quickfox
  $ hg add quickfox
  skipping invalid path 'he\rllo'
  skipping invalid path 'hell\no'
  $ hg ci -m 2
  skipping invalid path 'he\rllo'
  skipping invalid path 'hell\no'
  $ A=`printf 'quick\rfox'`
  $ hg cp quickfox "$A"
  skipping invalid path 'he\rllo'
  skipping invalid path 'hell\no'
  abort: '\n' and '\r' disallowed in filenames: 'quick\rfox'
  [255]
  $ hg mv quickfox "$A"
  skipping invalid path 'he\rllo'
  skipping invalid path 'hell\no'
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
  skipping invalid filename: 'foo
  bar.baz'
  skipping invalid filename: 'foo
  bar'

  $ cd ..
