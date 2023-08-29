#debugruntest-compatible

Prepare a repo

  $ newrepo
  $ setconfig ui.allowemptycommit=1
  $ hg ci -m 'A: foo bar'
  $ hg ci -m 'B: bar-baz'
  $ hg ci -m "$(printf 'C: multi line\nfoo bar baz 2nd line')"

  $ function log() {
  >   hg log -T '{desc|firstline}\n' -r "$1"
  > }

Select by word

  $ log "foo"
  A: foo bar
  hint[match-title]: commit matched by title from 'foo'
   (if you want to disable title matching, run 'hg config --edit experimental.titles-namespace=false')
  hint[hint-ack]: use 'hg hint --ack match-title' to silence these hints

  $ hg hint --ack match-title -q

  $ log "baz"
  B: bar-baz

  $ log "A"
  A: foo bar

  $ log "'B:'"
  B: bar-baz

Match the "max" commit

  $ log bar
  B: bar-baz

Case insensitive

  $ log 'FoO'
  A: foo bar

Not by an incomplete word

  $ log "fo"
  abort: unknown revision 'fo'!
  [255]

Select by words

  $ log "foo bar"
  A: foo bar

  $ log "bar-baz"
  B: bar-baz

Not by non-title (2nd line)

  $ log 2nd
  abort: unknown revision '2nd'!
  [255]

Embed in a revset expression

  $ log foo::baz
  A: foo bar
  B: bar-baz

  $ log '"foo bar"::"bar-baz"'
  A: foo bar
  B: bar-baz

Can be disabled

  $ setconfig experimental.titles-namespace=false
  $ log foo
  abort: unknown revision 'foo'!
  [255]
