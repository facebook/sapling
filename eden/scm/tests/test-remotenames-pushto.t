
Set up extension and repos

  $ eagerepo
  $ setconfig phases.publish=false
For convenience. The test was written without selectivepull and uses the "@" bookmark.
  $ setconfig remotenames.selectivepulldefault=master,@

  $ newclientrepo repo2 repo1

Test that anonymous heads are disallowed by default

  $ echo a > a
  $ sl add a
  $ sl commit -m a
  $ sl push
  pushing to test:repo1
  searching for changes
  abort: push would create new anonymous heads (cb9a9f314b8b)
  (use --allow-anon to override this warning)
  [255]

Test that config changes what is pushed by default

  $ echo b > b
  $ sl add b
  $ sl commit -m b
  $ sl up ".^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ sl add c
  $ sl commit -m c
  $ sl push -r 'head()'
  pushing to test:repo1
  searching for changes
  abort: push would create new anonymous heads (d2ae7f538514, d36c0562f908)
  (use --allow-anon to override this warning)
  [255]
  $ sl push -r .
  pushing to test:repo1
  searching for changes
  abort: push would create new anonymous heads (d36c0562f908)
  (use --allow-anon to override this warning)
  [255]
  $ sl debugstrip d36c0562f908 d2ae7f538514
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test that config allows anonymous heads to be pushed

  $ sl push --config remotenames.pushanonheads=True
  pushing to test:repo1
  searching for changes

Test that forceto works

  $ setconfig remotenames.forceto=true
  $ sl push
  abort: must specify --to when pushing
  (see configuration option remotenames.forceto)
  [255]

Test that --to limits other options

  $ echo b >> a
  $ sl commit -m b
  $ sl push --to @ --rev . --rev ".^"
  abort: --to requires exactly one rev to push
  (use --rev BOOKMARK or omit --rev for current commit (.))
  [255]
  $ sl push --to @ --bookmark foo
  abort: do not specify --to/-t and --bookmark/-B at the same time
  [255]

Test that --create is required to create new bookmarks

  $ sl push --to @
  pushing rev 1846eede8b68 to destination test:repo1 bookmark @
  searching for changes
  abort: not creating new remote bookmark
  (use --create to create a new bookmark)
  [255]
  $ sl push --to @ --create
  pushing rev 1846eede8b68 to destination test:repo1 bookmark @
  searching for changes
  exporting bookmark @

Test that --non-forward-move is required to move bookmarks to odd locations

  $ sl push --to @
  pushing rev 1846eede8b68 to destination test:repo1 bookmark @
  searching for changes
  remote bookmark already points at pushed rev
  $ sl push --to @ -r ".^"
  pushing rev cb9a9f314b8b to destination test:repo1 bookmark @
  searching for changes
  abort: pushed rev is not in the foreground of remote bookmark
  (use --non-forward-move flag to complete arbitrary moves)
  [255]
  $ sl up ".^"
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> a
  $ sl commit -m c
  $ sl push --to @
  pushing rev cc61aa6be3dc to destination test:repo1 bookmark @
  searching for changes
  abort: pushed rev is not in the foreground of remote bookmark
  (use --non-forward-move flag to complete arbitrary moves)
  [255]

Test that --non-forward-move allows moving bookmark around arbitrarily

  $ sl book -r 'desc(b)' headb
  $ sl book -r 'desc(c)' headc
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  c headc
  │
  │ o  b headb remote/@
  ├─╯
  o  a
  
  $ sl push --to @ -r headb
  pushing rev 1846eede8b68 to destination test:repo1 bookmark @
  searching for changes
  remote bookmark already points at pushed rev
  $ sl push --to @ -r headb
  pushing rev 1846eede8b68 to destination test:repo1 bookmark @
  searching for changes
  remote bookmark already points at pushed rev
  $ sl push --to @ -r headc
  pushing rev cc61aa6be3dc to destination test:repo1 bookmark @
  searching for changes
  abort: pushed rev is not in the foreground of remote bookmark
  (use --non-forward-move flag to complete arbitrary moves)
  [255]
  $ sl push --to @ -r headc --non-forward-move
  pushing rev cc61aa6be3dc to destination test:repo1 bookmark @
  searching for changes
  updating bookmark @
  $ sl push --to @ -r 'desc(a)'
  pushing rev cb9a9f314b8b to destination test:repo1 bookmark @
  searching for changes
  abort: pushed rev is not in the foreground of remote bookmark
  (use --non-forward-move flag to complete arbitrary moves)
  [255]
  $ sl push --to @ -r 'desc(a)' --non-forward-move
  pushing rev cb9a9f314b8b to destination test:repo1 bookmark @
  searching for changes
  updating bookmark @
  $ sl push --to @ -r headb
  pushing rev 1846eede8b68 to destination test:repo1 bookmark @
  searching for changes
  updating bookmark @

Test that local must have rev of remote to push --to without --non-forward-move

  $ sl up -r 'desc(a)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl debugstrip -r headb
  $ sl book -d headb
  $ sl push --to @ -r headc
  pushing rev cc61aa6be3dc to destination test:repo1 bookmark @
  searching for changes
  abort: remote bookmark @ revision 1846eede8b6886d8cc8a88c96a687b7fe8f3b9d1 is not in local repo
  (pull and merge or rebase or use --non-forward-move)
  [255]


Test that rebasing and pushing works as expected

  $ sl pull
  pulling from test:repo1
  searching for changes
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  o  c headc
  │
  │ o  b  remote/@
  ├─╯
  @  a
  
  $ sl --config extensions.rebase= rebase -d remote/@ -s headc 2>&1 | grep -v "^warning:" | grep -v incomplete
  rebasing cc61aa6be3dc "c" (headc)
  merging a
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  $ echo "a" > a
  $ echo "b" >> a
  $ echo "c" >> a
  $ sl resolve --mark a
  (no more unresolved files)
  continue: sl rebase --continue
  $ sl --config extensions.rebase= rebase --continue
  rebasing cc61aa6be3dc "c" (headc)
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  o  c headc
  │
  o  b  remote/@
  │
  @  a
  
  $ sl up headc
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark headc)
  $ sl push --to @
  pushing rev 6683576730c5 to destination test:repo1 bookmark @
  searching for changes
  updating bookmark @
  $ sl log -G -T '{desc} {bookmarks} {remotebookmarks}\n'
  @  c headc remote/@
  │
  o  b
  │
  o  a
  
# Evolve related tests removed. see https://fburl.com/evolveeol
