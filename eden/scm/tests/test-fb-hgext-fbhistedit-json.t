Tests JSON Input support for histedit

  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fbhistedit=
  > histedit=
  > rebase=
  > [experimental]
  > evolution=createmarkers
  > EOF

  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  > }

  $ initrepo

log before edit

  $ hg log --graph -T "{rev}:{node|short} {desc}"
  @  2:177f92b77385 c
  |
  o  1:d2ae7f538514 b
  |
  o  0:cb9a9f314b8b a
  

passing a json with invalid format

  $ cat >> input <<EOF
  > {
  >    "rebase": [
  >                 {"action": "foo", "node": "cb9a9f314b8b"},
  >                 {"action": "pick", "node": "d2ae7f538514"},
  >                 {"action": "pick", "node": "177f92b77385"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands input
  invalid JSON format, falling back to normal parsing
  hg: parse error: malformed line "{"
  [255]

  $ cat >> input2 <<EOF
  > {
  >    "histedit": [
  >                 {"node": "cb9a9f314b8b"},
  >                 {"action": "pick", "node": "d2ae7f538514"},
  >                 {"action": "pick", "node": "177f92b77385"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands input2
  invalid JSON format, falling back to normal parsing
  hg: parse error: malformed line "{"
  [255]

  $ cat >> input3 <<EOF
  > {
  >    "histedit": [
  >                 {"action": "pick", "node": "cb9a9f314b8b"},
  >                 {"action": "pick"},
  >                 {"action": "pick", "node": "177f92b77385"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands input3
  invalid JSON format, falling back to normal parsing
  hg: parse error: malformed line "{"
  [255]

passing a json with invalid action

  $ cat >> foo <<EOF
  > {
  >    "histedit": [
  >                 {"action": "foo", "node": "cb9a9f314b8b"},
  >                 {"action": "pick", "node": "d2ae7f538514"},
  >                 {"action": "pick", "node": "177f92b77385"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands foo
  hg: parse error: unknown action "foo"
  [255]

passing a json with invalid node
  $ cat >> bar <<EOF
  > {
  >    "histedit": [
  >                 {"action": "pick", "node": "123456abcdef"},
  >                 {"action": "pick", "node": "d2ae7f538514"},
  >                 {"action": "pick", "node": "177f92b77385"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands bar
  hg: parse error: unknown changeset 123456abcdef listed
  [255]

running histedit with a valid json file

  $ cat >> a.json <<EOF
  > {
  >    "histedit": [
  >                 {"action": "pick", "node": "cb9a9f314b8b"},
  >                 {"action": "exec", "command": "hg exp"},
  >                 {"action": "pick", "node": "177f92b77385"},
  >                 {"action": "exec", "command": "hg exp"},
  >                 {"action": "pick", "node": "d2ae7f538514"},
  >                 {"action": "exec", "command": "hg exp"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands a.json
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  # Parent  0000000000000000000000000000000000000000
  a
  
  diff -r 000000000000 -r cb9a9f314b8b a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +a
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID b346ab9a313db8537ecf96fca3ca3ca984ef3bd7
  # Parent  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  c
  
  diff -r cb9a9f314b8b -r b346ab9a313d c
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/c	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +c
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 503d1c1b466881fd108c7d4d26e78b510df14350
  # Parent  b346ab9a313db8537ecf96fca3ca3ca984ef3bd7
  b
  
  diff -r b346ab9a313d -r 503d1c1b4668 b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +b

log after histedit

  $ hg log --graph -T "{rev}:{node|short} {desc}"
  @  4:503d1c1b4668 b
  |
  o  3:b346ab9a313d c
  |
  o  0:cb9a9f314b8b a
  
testing with abbreviated/small verbs

  $ cat >> small <<EOF
  > {
  >    "histedit": [
  >                 {"action": "p", "node": "cb9a9f314b8b"},
  >                 {"action": "p", "node": "503d1c1b4668"},
  >                 {"action": "p", "node": "b346ab9a313d"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands small

  $ hg log --graph -T "{rev}:{node} {desc}"
  @  6:573a8c672aaf44d2cf3f9467e5463f51f7414084 c
  |
  o  5:85032a8e4f13e773c4075d7c006e0f1bc1c63967 b
  |
  o  0:cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b a
  
more testing with full hashes

  $ cat >> b.json <<EOF
  > {
  >    "histedit": [
  >                 {"action": "pick", "node": "cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b"},
  >                 {"action": "pick", "node": "85032a8e4f13e773c4075d7c006e0f1bc1c63967"},
  >                 {"action": "roll", "node": "573a8c672aaf44d2cf3f9467e5463f51f7414084"}
  >                ]
  > }
  > EOF

  $ hg histedit --commands b.json

  $ hg log --graph -T "{rev}:{node|short} {desc}"
  @  8:04e1eac0d294 b
  |
  o  0:cb9a9f314b8b a
  
