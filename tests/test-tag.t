  $ hg init test
  $ cd test

  $ echo a > a
  $ hg add a
  $ hg commit -m "test"
  $ hg history
  changeset:   0:acb14030fe0a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

  $ hg tag ' '
  abort: tag names cannot consist entirely of whitespace
  [255]

  $ hg tag "bleah"
  $ hg history
  changeset:   1:d4f0d2909abc
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag bleah for changeset acb14030fe0a
  
  changeset:   0:acb14030fe0a
  tag:         bleah
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  

  $ echo foo >> .hgtags
  $ hg tag "bleah2"
  abort: working copy of .hgtags is changed (please commit .hgtags manually)
  [255]

  $ hg revert .hgtags
  $ hg tag -r 0 x y z y y z
  abort: tag names must be unique
  [255]
  $ hg tag tap nada dot tip null .
  abort: the name 'tip' is reserved
  [255]
  $ hg tag "bleah"
  abort: tag 'bleah' already exists (use -f to force)
  [255]
  $ hg tag "blecch" "bleah"
  abort: tag 'bleah' already exists (use -f to force)
  [255]

  $ hg tag --remove "blecch"
  abort: tag 'blecch' does not exist
  [255]
  $ hg tag --remove "bleah" "blecch" "blough"
  abort: tag 'blecch' does not exist
  [255]

  $ hg tag -r 0 "bleah0"
  $ hg tag -l -r 1 "bleah1"
  $ hg tag gack gawk gorp
  $ hg tag -f gack
  $ hg tag --remove gack gorp

  $ hg tag "bleah "
  abort: tag 'bleah' already exists (use -f to force)
  [255]
  $ hg tag " bleah"
  abort: tag 'bleah' already exists (use -f to force)
  [255]
  $ hg tag " bleah"
  abort: tag 'bleah' already exists (use -f to force)
  [255]
  $ hg tag -r 0 "  bleahbleah  "
  $ hg tag -r 0 " bleah bleah "

  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 bleah
  acb14030fe0a21b60322c440ad2d20cf7685a376 bleah0
  336fccc858a4eb69609a291105009e484a6b6b8d gack
  336fccc858a4eb69609a291105009e484a6b6b8d gawk
  336fccc858a4eb69609a291105009e484a6b6b8d gorp
  336fccc858a4eb69609a291105009e484a6b6b8d gack
  799667b6f2d9b957f73fa644a918c2df22bab58f gack
  799667b6f2d9b957f73fa644a918c2df22bab58f gack
  0000000000000000000000000000000000000000 gack
  336fccc858a4eb69609a291105009e484a6b6b8d gorp
  0000000000000000000000000000000000000000 gorp
  acb14030fe0a21b60322c440ad2d20cf7685a376 bleahbleah
  acb14030fe0a21b60322c440ad2d20cf7685a376 bleah bleah

  $ cat .hg/localtags
  d4f0d2909abc9290e2773c08837d70c1794e3f5a bleah1

tagging on a non-head revision

  $ hg update 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg tag -l localblah
  $ hg tag "foobar"
  abort: not at a branch head (use -f to force)
  [255]
  $ hg tag -f "foobar"
  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  $ cat .hg/localtags
  d4f0d2909abc9290e2773c08837d70c1794e3f5a bleah1
  acb14030fe0a21b60322c440ad2d20cf7685a376 localblah

  $ hg tag -l 'xx
  > newline'
  abort: '\n' cannot be used in a tag name
  [255]
  $ hg tag -l 'xx:xx'
  abort: ':' cannot be used in a tag name
  [255]

cloning local tags

  $ cd ..
  $ hg -R test log -r0:5
  changeset:   0:acb14030fe0a
  tag:         bleah
  tag:         bleah bleah
  tag:         bleah0
  tag:         bleahbleah
  tag:         foobar
  tag:         localblah
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  
  changeset:   1:d4f0d2909abc
  tag:         bleah1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag bleah for changeset acb14030fe0a
  
  changeset:   2:336fccc858a4
  tag:         gawk
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag bleah0 for changeset acb14030fe0a
  
  changeset:   3:799667b6f2d9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag gack, gawk, gorp for changeset 336fccc858a4
  
  changeset:   4:154eeb7c0138
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag gack for changeset 799667b6f2d9
  
  changeset:   5:b4bb47aaff09
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Removed tag gack, gorp
  
  $ hg clone -q -rbleah1 test test1
  $ hg -R test1 parents --style=compact
  1[tip]   d4f0d2909abc   1970-01-01 00:00 +0000   test
    Added tag bleah for changeset acb14030fe0a
  
  $ hg clone -q -r5 test#bleah1 test2
  $ hg -R test2 parents --style=compact
  5[tip]   b4bb47aaff09   1970-01-01 00:00 +0000   test
    Removed tag gack, gorp
  
  $ hg clone -q -U test#bleah1 test3
  $ hg -R test3 parents --style=compact

  $ cd test

Issue601: hg tag doesn't do the right thing if .hgtags or localtags
doesn't end with EOL

  $ python << EOF
  > f = file('.hg/localtags'); last = f.readlines()[-1][:-1]; f.close()
  > f = file('.hg/localtags', 'w'); f.write(last); f.close()
  > EOF
  $ cat .hg/localtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 localblah
  $ hg tag -l localnewline
  $ cat .hg/localtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 localblah
  c2899151f4e76890c602a2597a650a72666681bf localnewline
  

  $ python << EOF
  > f = file('.hgtags'); last = f.readlines()[-1][:-1]; f.close()
  > f = file('.hgtags', 'w'); f.write(last); f.close()
  > EOF
  $ hg ci -m'broken manual edit of .hgtags'
  $ cat .hgtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  $ hg tag newline
  $ cat .hgtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  a0eea09de1eeec777b46f2085260a373b2fbc293 newline
  

tag and branch using same name

  $ hg branch tag-and-branch-same-name
  marked working directory as branch tag-and-branch-same-name
  $ hg ci -m"discouraged"
  $ hg tag tag-and-branch-same-name
  warning: tag tag-and-branch-same-name conflicts with existing branch name

test custom commit messages

  $ cat > editor << '__EOF__'
  > #!/bin/sh
  > echo "custom tag message" > "$1"
  > echo "second line" >> "$1"
  > __EOF__
  $ chmod +x editor
  $ HGEDITOR="'`pwd`'"/editor hg tag custom-tag -e
  $ hg log -l1 --template "{desc}\n"
  custom tag message
  second line


local tag with .hgtags modified

  $ hg tag hgtags-modified
  $ hg rollback
  repository tip rolled back to revision 13 (undo commit)
  working directory now based on revision 13
  $ hg st
  M .hgtags
  ? .hgtags.orig
  ? editor
  $ hg tag --local baz
  $ hg revert --no-backup .hgtags


tagging when at named-branch-head that's not a topo-head

  $ hg up default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge -t internal:local
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge named branch'
  $ hg up 13
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg tag new-topo-head


tagging on null rev

  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg tag nullrev
  abort: not at a branch head (use -f to force)
  [255]

  $ hg init empty
  $ hg tag -R empty nullrev

  $ cd ..

tagging on an uncommitted merge (issue2542)

  $ hg init repo-tag-uncommitted-merge
  $ cd repo-tag-uncommitted-merge
  $ echo c1 > f1
  $ hg ci -Am0
  adding f1
  $ echo c2 > f2
  $ hg ci -Am1
  adding f2
  $ hg co -q 0
  $ hg branch b1
  marked working directory as branch b1
  $ hg ci -m2
  $ hg up default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge b1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg tag t1
  abort: uncommitted merge
  [255]
  $ hg status
  $ hg tag --rev 1 t2
  abort: uncommitted merge
  [255]
  $ hg tag --rev 1 --local t3
  $ hg tags -v
  tip                                2:2a156e8887cc
  t3                                 1:c3adabd1a5f4 local

  $ cd ..
