#chg-compatible

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > [experimental]
  > evolution = all
  > EOF

  $ hg init
  $ drawdag <<'EOS'
  > B C   # amend: B -> C
  > |/
  > A
  > EOS

  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  o  0 A
  
  $ hg log -G -T '{rev} {desc}\n' --hidden
  o  2 C
  |
  | x  1 B
  |/
  o  0 A
  
Changing working copy parent pins a node

  $ hg update 1 --hidden -q
  $ hg update 0 -q
  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  | x  1 B
  |/
  @  0 A
  
Strip/prune unpins a node

  $ hg prune 1 -q
  hint[strip-hide]: 'hg strip' may be deprecated in the future - use 'hg hide' instead
  hint[hint-ack]: use 'hg hint --ack strip-hide' to silence these hints
  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  @  0 A
  
Bookmark pins nodes even after removed

  $ hg bookmark -ir 1 BOOK --hidden -q
  $ hg bookmark -d BOOK -q
  $ hg log -G -T '{rev} {desc}\n'
  o  2 C
  |
  | x  1 B
  |/
  @  0 A
  
Check blackbox logs

  $ hg blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":["or","command","command_finish","pinnednodes"]}}'
  [legacy][command] debugdrawdag
  [legacy][pinnednodes] pinnednodes: ['debugdrawdag'] newpin=['112478962961'] newunpin=['112478962961'] before=[] after=[]
  [legacy][command_finish] debugdrawdag exited 0 after 0.00 seconds
  [legacy][command] bookmarks -T '{bookmark}={node}\n'
  [legacy][command_finish] bookmarks -T '{bookmark}={node}\n' exited 0 after 0.00 seconds
  [legacy][command] book -T '{bookmark} '
  [legacy][command_finish] book -T '{bookmark} ' exited 0 after 0.00 seconds
  [legacy][command] book -fd A B C
  [legacy][command_finish] book -fd A B C exited 0 after 0.00 seconds
  [legacy][command] log -G -T '{rev} {desc}\n'
  [legacy][command_finish] log -G -T '{rev} {desc}\n' exited 0 after 0.00 seconds
  [legacy][command] log -G -T '{rev} {desc}\n' --hidden
  [legacy][command_finish] log -G -T '{rev} {desc}\n' --hidden exited 0 after 0.00 seconds
  [legacy][command] update 1 --hidden -q
  [legacy][pinnednodes] pinnednodes: ['update', '1', '--hidden', '-q'] newpin=['112478962961'] newunpin=[] before=[] after=['112478962961']
  [legacy][command_finish] update 1 --hidden -q exited 0 after 0.00 seconds
  [legacy][command] update 0 -q
  [legacy][command_finish] update 0 -q exited 0 after 0.00 seconds
  [legacy][command] log -G -T '{rev} {desc}\n'
  [legacy][command_finish] log -G -T '{rev} {desc}\n' exited 0 after 0.00 seconds
  [legacy][command] prune 1 -q
  [legacy][pinnednodes] pinnednodes: ['prune', '1', '-q'] newpin=[] newunpin=['112478962961'] before=['112478962961'] after=[]
  [legacy][command_finish] prune 1 -q exited 0 after 0.00 seconds
  [legacy][command] log -G -T '{rev} {desc}\n'
  [legacy][command_finish] log -G -T '{rev} {desc}\n' exited 0 after 0.00 seconds
  [legacy][command] bookmark -ir 1 BOOK --hidden -q
  [legacy][pinnednodes] pinnednodes: ['bookmark', '-ir', '1', 'BOOK', '--hidden', '-q'] newpin=['112478962961'] newunpin=[] before=[] after=['112478962961']
  [legacy][command_finish] bookmark -ir 1 BOOK --hidden -q exited 0 after 0.00 seconds
  [legacy][command] bookmark -d BOOK -q
  [legacy][command_finish] bookmark -d BOOK -q exited 0 after 0.00 seconds
  [legacy][command] log -G -T '{rev} {desc}\n'
  [legacy][command_finish] log -G -T '{rev} {desc}\n' exited 0 after 0.00 seconds
  [legacy][command] blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":["or","command","command_finish","pinnednodes"]}}'

The order matters - putting bookmarks or moving working copy on non-obsoleted
commits do not pin them. Test this using "debugobsolete" which will not call
"createmarkers".

Obsolete working copy, and move working copy away should make things disappear

  $ rm -rf .hg && hg init && drawdag <<'EOS'
  > C E
  > | |
  > B D
  > |/
  > A
  > EOS

  $ hg up -q $E
  $ hg debugobsolete `HGPLAIN=1 hg log -r $E -T '{node}'`
  obsoleted 1 changesets
  $ hg log -G -T '{rev} {desc}\n'
  @  4 E
  |
  | o  3 C
  | |
  o |  2 D
  | |
  | o  1 B
  |/
  o  0 A
  
  $ hg debugobsolete `HGPLAIN=1 hg log -r $D -T '{node}'`
  obsoleted 1 changesets
  $ hg log -G -T '{rev} {desc}\n'
  @  4 E
  |
  | o  3 C
  | |
  x |  2 D
  | |
  | o  1 B
  |/
  o  0 A
  
  $ hg update -q $C
  $ hg log -G -T '{rev} {desc}\n'
  @  3 C
  |
  o  1 B
  |
  o  0 A
  
Having a bookmark on a commit, obsolete the commit, remove the bookmark

  $ rm -rf .hg && hg init && drawdag <<'EOS'
  > C E
  > | |
  > B D
  > |/
  > A
  > EOS

  $ hg bookmark -i book-e -r $E
  $ hg debugobsolete `HGPLAIN=1 hg log -r $D -T '{node}'`
  obsoleted 1 changesets
  $ hg debugobsolete `HGPLAIN=1 hg log -r $E -T '{node}'`
  obsoleted 1 changesets
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  x  4 E book-e
  |
  | o  3 C
  | |
  x |  2 D
  | |
  | o  1 B
  |/
  o  0 A
  
  $ hg bookmark -d book-e
  $ hg log -G -T '{rev} {desc} {bookmarks}\n'
  o  3 C
  |
  o  1 B
  |
  o  0 A
  
Uncommit and hiddenoverride. This is uncommon but the last uncommit should make
"A" invisible:

  $ newrepo
  $ drawdag <<'EOS'
  >   B
  >   |
  >   A
  >   |
  >   Z
  > EOS

  $ hg up $A
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg uncommit
  $ hg log -T '{desc}' -G
  o  B
  |
  x  A
  |
  @  Z
  
  $ hg up -C $B
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg uncommit
  $ hg log -T '{desc}' -G
  @  A
  |
  o  Z
  
  $ hg up -C .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg uncommit
  $ hg log -T '{desc}' -G
  @  Z
  
