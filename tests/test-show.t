  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > show =
  > EOF

No arguments shows available views

  $ hg init empty
  $ cd empty
  $ hg show
  available views:
  
  bookmarks -- bookmarks and their associated changeset
  stack -- current line of work
  work -- changesets that aren't finished
  
  abort: no view requested
  (use "hg show VIEW" to choose a view)
  [255]

`hg help show` prints available views

  $ hg help show
  hg show VIEW
  
  show various repository information
  
      A requested view of repository data is displayed.
  
      If no view is requested, the list of available views is shown and the
      command aborts.
  
      Note:
         There are no backwards compatibility guarantees for the output of this
         command. Output may change in any future Mercurial release.
  
         Consumers wanting stable command output should specify a template via
         "-T/--template".
  
      List of available views:
  
      bookmarks   bookmarks and their associated changeset
  
      stack       current line of work
  
      work        changesets that aren't finished
  
  (use 'hg help -e show' to show help for the show extension)
  
  options:
  
   -T --template TEMPLATE display with template
  
  (some details hidden, use --verbose to show complete help)

Unknown view prints error

  $ hg show badview
  abort: unknown view: badview
  (run "hg show" to see available views)
  [255]

HGPLAIN results in abort

  $ HGPLAIN=1 hg show bookmarks
  abort: must specify a template in plain mode
  (invoke with -T/--template to control output format)
  [255]

But not if a template is specified

  $ HGPLAIN=1 hg show bookmarks -T '{bookmark}\n'
  (no bookmarks set)

  $ cd ..

bookmarks view with no bookmarks prints empty message

  $ hg init books
  $ cd books
  $ touch f0
  $ hg -q commit -A -m initial

  $ hg show bookmarks
  (no bookmarks set)

bookmarks view shows bookmarks in an aligned table

  $ echo book1 > f0
  $ hg commit -m 'commit for book1'
  $ echo book2 > f0
  $ hg commit -m 'commit for book2'

  $ hg bookmark -r 1 book1
  $ hg bookmark a-longer-bookmark

  $ hg show bookmarks
  * a-longer-bookmark    7b57
    book1                b757

A custom bookmarks template works

  $ hg show bookmarks -T '{node} {bookmark} {active}\n'
  7b5709ab64cbc34da9b4367b64afff47f2c4ee83 a-longer-bookmark True
  b757f780b8ffd71267c6ccb32e0882d9d32a8cc0 book1 False

bookmarks JSON works

  $ hg show bookmarks -T json
  [
   {
    "active": true,
    "bookmark": "a-longer-bookmark",
    "longestbookmarklen": 17,
    "node": "7b5709ab64cbc34da9b4367b64afff47f2c4ee83",
    "nodelen": 4
   },
   {
    "active": false,
    "bookmark": "book1",
    "longestbookmarklen": 17,
    "node": "b757f780b8ffd71267c6ccb32e0882d9d32a8cc0",
    "nodelen": 4
   }
  ]

JSON works with no bookmarks

  $ hg book -d a-longer-bookmark
  $ hg book -d book1
  $ hg show bookmarks -T json
  [
  ]

commands.show.aliasprefix aliases values to `show <view>`

  $ hg --config commands.show.aliasprefix=s sbookmarks
  (no bookmarks set)

  $ hg --config commands.show.aliasprefix=sh shwork
  @  7b57 commit for book2
  o  b757 commit for book1
  o  ba59 initial

  $ hg --config commands.show.aliasprefix='s sh' swork
  @  7b57 commit for book2
  o  b757 commit for book1
  o  ba59 initial

  $ hg --config commands.show.aliasprefix='s sh' shwork
  @  7b57 commit for book2
  o  b757 commit for book1
  o  ba59 initial

The aliases don't appear in `hg config`

  $ hg --config commands.show.aliasprefix=s config alias
  [1]

Doesn't overwrite existing alias

  $ hg --config alias.swork='log -r .' --config commands.show.aliasprefix=s swork
  changeset:   2:7b5709ab64cb
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     commit for book2
  

  $ hg --config alias.swork='log -r .' --config commands.show.aliasprefix=s config alias
  alias.swork=log -r .

  $ cd ..
