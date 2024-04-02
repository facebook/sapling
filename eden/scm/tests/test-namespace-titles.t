#debugruntest-compatible

#require no-eden

  $ setconfig ui.allowemptycommit=1 \
  > remotenames.selectivepulldefault=main \
  > remotenames.hoist=remote \
  > remotenames.rename.default=remote

  $ enable remotenames

Prepare a repo

  $ newrepo
  $ hg ci -m 'A: foo bar'
  $ hg ci -m 'B: bar-baz'
  $ hg go -q 'desc("A: foo")'
  $ hg ci -m "$(printf 'C: multi line\nfoo bar baz 2nd line')"
  $ hg ci -m 'D: not public'

  $ function log() {
  >   hg log -T '{desc|firstline}\n' -r "$@"
  > }

Not by symbol

  $ log 'desc("bar-baz")::'
  B: bar-baz

Do not conflict with aliases or trigger hint messages

  $ log public '--config=revsetalias.public=public()'

  $ log 'public()'

  $ log 'public()' '--config=revsetalias.public()=.'
  D: not public

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

  $ log foo --config experimental.titles-namespace=false
  abort: unknown revision 'foo'!
  [255]

Does not conflict with autopull

  $ hg bookmark -r 'desc(D)' main
  $ hg clone -q "$PWD" "$TESTTMP/client1"

  $ cd "$TESTTMP/client1"
  $ hg go -q remote/main
  $ hg commit -m 'E: remote/foo'

  $ log 'remote/foo'
  pulling 'foo' from '$TESTTMP/repo1' (no-windows !)
  pulling 'foo' from '*\repo1' (glob) (windows !)
  abort: unknown revision 'remote/foo'!
  [255]

Disable autopull by disabling remote, then the titles namespace works again

  $ log 'remote/foo' --config paths.default=
  E: remote/foo

