  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > hook-track-tags=1
  > [hooks]
  > txnclose.track-tag=sh ${TESTTMP}/taghook.sh
  > EOF

  $ cat << EOF > taghook.sh
  > #!/bin/sh
  > # escape the "$" otherwise the test runner interpret it when writting the
  > # file...
  > if [ -n "\$HG_TAG_MOVED" ]; then
  >     echo 'hook: tag changes detected'
  >     sed 's/^/hook: /' .hg/changes/tags.changes
  > fi
  > EOF
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

(this tests also that editor is not invoked, if '--edit' is not
specified)

  $ HGEDITOR=cat hg tag "bleah"
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleah
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
  abort: working copy of .hgtags is changed
  (please commit .hgtags manually)
  [255]

  $ hg revert .hgtags
  $ hg tag -r 0 x y z y y z
  abort: tag names must be unique
  [255]
  $ hg tag tap nada dot tip
  abort: the name 'tip' is reserved
  [255]
  $ hg tag .
  abort: the name '.' is reserved
  [255]
  $ hg tag null
  abort: the name 'null' is reserved
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
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleah0
  $ hg tag -l -r 1 "bleah1"
  $ hg tag gack gawk gorp
  hook: tag changes detected
  hook: +A 336fccc858a4eb69609a291105009e484a6b6b8d gack
  hook: +A 336fccc858a4eb69609a291105009e484a6b6b8d gawk
  hook: +A 336fccc858a4eb69609a291105009e484a6b6b8d gorp
  $ hg tag -f gack
  hook: tag changes detected
  hook: -M 336fccc858a4eb69609a291105009e484a6b6b8d gack
  hook: +M 799667b6f2d9b957f73fa644a918c2df22bab58f gack
  $ hg tag --remove gack gorp
  hook: tag changes detected
  hook: -R 799667b6f2d9b957f73fa644a918c2df22bab58f gack
  hook: -R 336fccc858a4eb69609a291105009e484a6b6b8d gorp

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
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleahbleah
  $ hg tag -r 0 " bleah bleah "
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleah bleah

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
  abort: working directory is not at a branch head (use -f to force)
  [255]
  $ hg tag -f "foobar"
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  $ cat .hg/localtags
  d4f0d2909abc9290e2773c08837d70c1794e3f5a bleah1
  acb14030fe0a21b60322c440ad2d20cf7685a376 localblah

  $ hg tag -l 'xx
  > newline'
  abort: '\n' cannot be used in a name
  [255]
  $ hg tag -l 'xx:xx'
  abort: ':' cannot be used in a name
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
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleah
  $ hg -R test1 parents --style=compact
  1[tip]   d4f0d2909abc   1970-01-01 00:00 +0000   test
    Added tag bleah for changeset acb14030fe0a
  
  $ hg clone -q -r5 test#bleah1 test2
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleah
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleah0
  hook: +A 336fccc858a4eb69609a291105009e484a6b6b8d gawk
  $ hg -R test2 parents --style=compact
  5[tip]   b4bb47aaff09   1970-01-01 00:00 +0000   test
    Removed tag gack, gorp
  
  $ hg clone -q -U test#bleah1 test3
  hook: tag changes detected
  hook: +A acb14030fe0a21b60322c440ad2d20cf7685a376 bleah
  $ hg -R test3 parents --style=compact

  $ cd test

Issue601: hg tag doesn't do the right thing if .hgtags or localtags
doesn't end with EOL

  $ $PYTHON << EOF
  > f = file('.hg/localtags'); last = f.readlines()[-1][:-1]; f.close()
  > f = file('.hg/localtags', 'w'); f.write(last); f.close()
  > EOF
  $ cat .hg/localtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 localblah
  $ hg tag -l localnewline
  $ cat .hg/localtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 localblah
  c2899151f4e76890c602a2597a650a72666681bf localnewline
  

  $ $PYTHON << EOF
  > f = file('.hgtags'); last = f.readlines()[-1][:-1]; f.close()
  > f = file('.hgtags', 'w'); f.write(last); f.close()
  > EOF
  $ hg ci -m'broken manual edit of .hgtags'
  $ cat .hgtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  $ hg tag newline
  hook: tag changes detected
  hook: +A a0eea09de1eeec777b46f2085260a373b2fbc293 newline
  $ cat .hgtags; echo
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  a0eea09de1eeec777b46f2085260a373b2fbc293 newline
  

tag and branch using same name

  $ hg branch tag-and-branch-same-name
  marked working directory as branch tag-and-branch-same-name
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m"discouraged"
  $ hg tag tag-and-branch-same-name
  warning: tag tag-and-branch-same-name conflicts with existing branch name
  hook: tag changes detected
  hook: +A fc93d2ea1cd78e91216c6cfbbf26747c10ce11ae tag-and-branch-same-name

test custom commit messages

  $ cat > editor.sh << '__EOF__'
  > echo "==== before editing"
  > cat "$1"
  > echo "===="
  > echo "custom tag message" > "$1"
  > echo "second line" >> "$1"
  > __EOF__

at first, test saving last-message.txt

(test that editor is not invoked before transaction starting)

  $ cat > .hg/hgrc << '__EOF__'
  > [hooks]
  > # this failure occurs before editor invocation
  > pretag.test-saving-lastmessage = false
  > __EOF__
  $ rm -f .hg/last-message.txt
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg tag custom-tag -e
  abort: pretag.test-saving-lastmessage hook exited with status 1
  [255]
  $ test -f .hg/last-message.txt
  [1]

(test that editor is invoked and commit message is saved into
"last-message.txt")

  $ cat >> .hg/hgrc << '__EOF__'
  > [hooks]
  > pretag.test-saving-lastmessage =
  > # this failure occurs after editor invocation
  > pretxncommit.unexpectedabort = false
  > __EOF__

(this tests also that editor is invoked, if '--edit' is specified,
regardless of '--message')

  $ rm -f .hg/last-message.txt
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg tag custom-tag -e -m "foo bar"
  ==== before editing
  foo bar
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'tag-and-branch-same-name'
  HG: changed .hgtags
  ====
  note: commit message saved in .hg/last-message.txt
  transaction abort!
  rollback completed
  abort: pretxncommit.unexpectedabort hook exited with status 1
  [255]
  $ cat .hg/last-message.txt
  custom tag message
  second line

  $ cat >> .hg/hgrc << '__EOF__'
  > [hooks]
  > pretxncommit.unexpectedabort =
  > __EOF__
  $ hg status .hgtags
  M .hgtags
  $ hg revert --no-backup -q .hgtags

then, test custom commit message itself

  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg tag custom-tag -e
  ==== before editing
  Added tag custom-tag for changeset 75a534207be6
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'tag-and-branch-same-name'
  HG: changed .hgtags
  ====
  hook: tag changes detected
  hook: +A 75a534207be6b03576e0c7a4fa5708d045f1c876 custom-tag
  $ hg log -l1 --template "{desc}\n"
  custom tag message
  second line


local tag with .hgtags modified

  $ hg tag hgtags-modified
  hook: tag changes detected
  hook: +A 0f26aaea6f74c3ed6c4aad8844403c9ba128d23a hgtags-modified
  $ hg rollback
  repository tip rolled back to revision 13 (undo commit)
  working directory now based on revision 13
  $ hg st
  M .hgtags
  ? .hgtags.orig
  ? editor.sh
  $ hg tag --local baz
  $ hg revert --no-backup .hgtags


tagging when at named-branch-head that's not a topo-head

  $ hg up default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge -t internal:local
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge named branch'
  hook: tag changes detected
  hook: -R acb14030fe0a21b60322c440ad2d20cf7685a376 bleah
  hook: -R acb14030fe0a21b60322c440ad2d20cf7685a376 bleah bleah
  hook: -R acb14030fe0a21b60322c440ad2d20cf7685a376 bleah0
  hook: -R acb14030fe0a21b60322c440ad2d20cf7685a376 bleahbleah
  hook: -R 336fccc858a4eb69609a291105009e484a6b6b8d gawk
  $ hg up 13
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg tag new-topo-head
  hook: tag changes detected
  hook: +A 0f26aaea6f74c3ed6c4aad8844403c9ba128d23a new-topo-head

tagging on null rev

  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg tag nullrev
  abort: working directory is not at a branch head (use -f to force)
  [255]

  $ hg init empty
  $ hg tag -R empty nullrev
  abort: cannot tag null revision
  [255]

  $ hg tag -R empty -r 00000000000 -f nulltag
  abort: cannot tag null revision
  [255]

issue5539: pruned tags do not appear in .hgtags

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > evolution=exchange
  > evolution.createmarkers=True
  > EOF
  $ hg up e4d483960b9b --quiet
  $ echo aaa >>a
  $ hg ci -maaa
  $ hg log -r . -T "{node}\n"
  743b3afd5aa69f130c246806e48ad2e699f490d2
  $ hg tag issue5539
  hook: tag changes detected
  hook: +A 743b3afd5aa69f130c246806e48ad2e699f490d2 issue5539
  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  a0eea09de1eeec777b46f2085260a373b2fbc293 newline
  743b3afd5aa69f130c246806e48ad2e699f490d2 issue5539
  $ hg log -r . -T "{node}\n"
  abeb261f0508ecebcd345ce21e7a25112df417aa
(mimic 'hg prune' command by obsoleting current changeset and then moving to its parent)
  $ hg debugobsolete abeb261f0508ecebcd345ce21e7a25112df417aa --record-parents
  obsoleted 1 changesets
  $ hg up ".^" --quiet
  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  a0eea09de1eeec777b46f2085260a373b2fbc293 newline
  $ echo bbb >>a
  $ hg ci -mbbb
  $ hg log -r . -T "{node}\n"
  089dd20da58cae34165c37b064539c6ac0c6a0dd
  $ hg tag issue5539
  hook: tag changes detected
  hook: -M 743b3afd5aa69f130c246806e48ad2e699f490d2 issue5539
  hook: +M 089dd20da58cae34165c37b064539c6ac0c6a0dd issue5539
  $ hg id
  0accf560a709 tip
  $ cat .hgtags
  acb14030fe0a21b60322c440ad2d20cf7685a376 foobar
  a0eea09de1eeec777b46f2085260a373b2fbc293 newline
  089dd20da58cae34165c37b064539c6ac0c6a0dd issue5539
  $ hg tags
  tip                               19:0accf560a709
  issue5539                         18:089dd20da58c
  new-topo-head                     13:0f26aaea6f74
  baz                               13:0f26aaea6f74
  custom-tag                        12:75a534207be6
  tag-and-branch-same-name          11:fc93d2ea1cd7
  newline                            9:a0eea09de1ee
  localnewline                       8:c2899151f4e7
  localblah                          0:acb14030fe0a
  foobar                             0:acb14030fe0a

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
  (branches are permanent and global, did you want a bookmark?)
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

commit hook on tag used to be run without write lock - issue3344

  $ hg init repo-tag
  $ touch repo-tag/test
  $ hg -R repo-tag commit -A -m "test"
  adding test
  $ hg init repo-tag-target
  $ cat > "$TESTTMP/issue3344.sh" <<EOF
  > hg push "$TESTTMP/repo-tag-target"
  > EOF
  $ hg -R repo-tag --config hooks.commit="sh ../issue3344.sh" tag tag
  hook: tag changes detected
  hook: +A be090ea6625635128e90f7d89df8beeb2bcc1653 tag
  pushing to $TESTTMP/repo-tag-target (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  hook: tag changes detected
  hook: +A be090ea6625635128e90f7d89df8beeb2bcc1653 tag

automatically merge resolvable tag conflicts (i.e. tags that differ in rank)
create two clones with some different tags as well as some common tags
check that we can merge tags that differ in rank

  $ hg init repo-automatic-tag-merge
  $ cd repo-automatic-tag-merge
  $ echo c0 > f0
  $ hg ci -A -m0
  adding f0
  $ hg tag tbase
  hook: tag changes detected
  hook: +A 6cee5c8f3e5b4ae1a3996d2f6489c3e08eb5aea7 tbase
  $ hg up -qr '.^'
  $ hg log -r 'wdir()' -T "{latesttagdistance}\n"
  1
  $ hg up -q
  $ hg log -r 'wdir()' -T "{latesttagdistance}\n"
  2
  $ cd ..
  $ hg clone repo-automatic-tag-merge repo-automatic-tag-merge-clone
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-automatic-tag-merge-clone
  $ echo c1 > f1
  $ hg ci -A -m1
  adding f1
  $ hg tag t1 t2 t3
  hook: tag changes detected
  hook: +A 4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t1
  hook: +A 4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  hook: +A 4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  $ hg tag --remove t2
  hook: tag changes detected
  hook: -R 4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  $ hg tag t5
  hook: tag changes detected
  hook: +A 875517b4806a848f942811a315a5bce30804ae85 t5
  $ echo c2 > f2
  $ hg ci -A -m2
  adding f2
  $ hg tag -f t3
  hook: tag changes detected
  hook: -M 4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  hook: +M 79505d5360b07e3e79d1052e347e73c02b8afa5b t3

  $ cd ../repo-automatic-tag-merge
  $ echo c3 > f3
  $ hg ci -A -m3
  adding f3
  $ hg tag -f t4 t5 t6
  hook: tag changes detected
  hook: +A 9aa4e1292a27a248f8d07339bed9931d54907be7 t4
  hook: +A 9aa4e1292a27a248f8d07339bed9931d54907be7 t5
  hook: +A 9aa4e1292a27a248f8d07339bed9931d54907be7 t6

  $ hg up -q '.^'
  $ hg log -r 'wdir()' -T "{changessincelatesttag} changes since {latesttag}\n"
  1 changes since t4:t5:t6
  $ hg log -r '.' -T "{changessincelatesttag} changes since {latesttag}\n"
  0 changes since t4:t5:t6
  $ echo c5 > f3
  $ hg log -r 'wdir()' -T "{changessincelatesttag} changes since {latesttag}\n"
  1 changes since t4:t5:t6
  $ hg up -qC

  $ hg tag --remove t5
  hook: tag changes detected
  hook: -R 9aa4e1292a27a248f8d07339bed9931d54907be7 t5
  $ echo c4 > f4
  $ hg log -r '.' -T "{changessincelatesttag} changes since {latesttag}\n"
  2 changes since t4:t6
  $ hg log -r '.' -T "{latesttag % '{latesttag}\n'}"
  t4
  t6
  $ hg log -r '.' -T "{latesttag('t4') % 'T: {tag}, C: {changes}, D: {distance}\n'}"
  T: t4, C: 2, D: 2
  $ hg log -r '.' -T "{latesttag('re:\d') % 'T: {tag}, C: {changes}, D: {distance}\n'}"
  T: t4, C: 2, D: 2
  T: t6, C: 2, D: 2
  $ hg log -r . -T '{join(latesttag(), "*")}\n'
  t4*t6
  $ hg ci -A -m4
  adding f4
  $ hg log -r 'wdir()' -T "{changessincelatesttag} changes since {latesttag}\n"
  4 changes since t4:t6
  $ hg tag t2
  hook: tag changes detected
  hook: +A 929bca7b18d067cbf3844c3896319a940059d748 t2
  $ hg tag -f t6
  hook: tag changes detected
  hook: -M 9aa4e1292a27a248f8d07339bed9931d54907be7 t6
  hook: +M 09af2ce14077a94effef208b49a718f4836d4338 t6

  $ cd ../repo-automatic-tag-merge-clone
  $ hg pull
  pulling from $TESTTMP/repo-automatic-tag-merge (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 6 changes to 3 files (+1 heads)
  new changesets 9aa4e1292a27:b325cc5b642c
  hook: tag changes detected
  hook: +A 929bca7b18d067cbf3844c3896319a940059d748 t2
  hook: +A 9aa4e1292a27a248f8d07339bed9931d54907be7 t4
  hook: -R 875517b4806a848f942811a315a5bce30804ae85 t5
  hook: +A 09af2ce14077a94effef208b49a718f4836d4338 t6
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge --tool internal:tagmerge
  merging .hgtags
  2 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status
  M .hgtags
  M f3
  M f4
  $ hg resolve -l
  R .hgtags
  $ cat .hgtags
  9aa4e1292a27a248f8d07339bed9931d54907be7 t4
  9aa4e1292a27a248f8d07339bed9931d54907be7 t6
  9aa4e1292a27a248f8d07339bed9931d54907be7 t6
  09af2ce14077a94effef208b49a718f4836d4338 t6
  6cee5c8f3e5b4ae1a3996d2f6489c3e08eb5aea7 tbase
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t1
  929bca7b18d067cbf3844c3896319a940059d748 t2
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  0000000000000000000000000000000000000000 t2
  875517b4806a848f942811a315a5bce30804ae85 t5
  9aa4e1292a27a248f8d07339bed9931d54907be7 t5
  9aa4e1292a27a248f8d07339bed9931d54907be7 t5
  0000000000000000000000000000000000000000 t5
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  79505d5360b07e3e79d1052e347e73c02b8afa5b t3

check that the merge tried to minimize the diff with the first merge parent

  $ hg diff --git -r 'p1()' .hgtags
  diff --git a/.hgtags b/.hgtags
  --- a/.hgtags
  +++ b/.hgtags
  @@ -1,9 +1,17 @@
  +9aa4e1292a27a248f8d07339bed9931d54907be7 t4
  +9aa4e1292a27a248f8d07339bed9931d54907be7 t6
  +9aa4e1292a27a248f8d07339bed9931d54907be7 t6
  +09af2ce14077a94effef208b49a718f4836d4338 t6
   6cee5c8f3e5b4ae1a3996d2f6489c3e08eb5aea7 tbase
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t1
  +929bca7b18d067cbf3844c3896319a940059d748 t2
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
   0000000000000000000000000000000000000000 t2
   875517b4806a848f942811a315a5bce30804ae85 t5
  +9aa4e1292a27a248f8d07339bed9931d54907be7 t5
  +9aa4e1292a27a248f8d07339bed9931d54907be7 t5
  +0000000000000000000000000000000000000000 t5
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
   79505d5360b07e3e79d1052e347e73c02b8afa5b t3

detect merge tag conflicts

  $ hg update -C -r tip
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg tag t7
  hook: tag changes detected
  hook: +A b325cc5b642c5b465bdbe8c09627cb372de3d47d t7
  $ hg update -C -r 'first(sort(head()))'
  3 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ printf "%s %s\n" `hg log -r . --template "{node} t7"` >> .hgtags
  $ hg commit -m "manually add conflicting t7 tag"
  hook: tag changes detected
  hook: -R 929bca7b18d067cbf3844c3896319a940059d748 t2
  hook: +A 875517b4806a848f942811a315a5bce30804ae85 t5
  hook: -M b325cc5b642c5b465bdbe8c09627cb372de3d47d t7
  hook: +M ea918d56be86a4afc5a95312e8b6750e1428d9d2 t7
  $ hg merge --tool internal:tagmerge
  merging .hgtags
  automatic .hgtags merge failed
  the following 1 tags are in conflict: t7
  automatic tag merging of .hgtags failed! (use 'hg resolve --tool :merge' or another merge tool of your choice)
  2 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg resolve -l
  U .hgtags
  $ cat .hgtags
  6cee5c8f3e5b4ae1a3996d2f6489c3e08eb5aea7 tbase
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t1
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  0000000000000000000000000000000000000000 t2
  875517b4806a848f942811a315a5bce30804ae85 t5
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  79505d5360b07e3e79d1052e347e73c02b8afa5b t3
  ea918d56be86a4afc5a95312e8b6750e1428d9d2 t7

  $ cd ..

handle the loss of tags

  $ hg clone repo-automatic-tag-merge-clone repo-merge-lost-tags
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-merge-lost-tags
  $ echo c5 > f5
  $ hg ci -A -m5
  adding f5
  $ hg tag -f t7
  hook: tag changes detected
  hook: -M ea918d56be86a4afc5a95312e8b6750e1428d9d2 t7
  hook: +M fd3a9e394ce3afb354a496323bf68ac1755a30de t7
  $ hg update -r 'p1(t7)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ printf '' > .hgtags
  $ hg commit -m 'delete all tags'
  created new head
  $ hg log -r 'max(t7::)'
  changeset:   17:ffe462b50880
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag t7 for changeset fd3a9e394ce3
  
  $ hg update -r 'max(t7::)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge -r tip --tool internal:tagmerge
  merging .hgtags
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg resolve -l
  R .hgtags
  $ cat .hgtags
  6cee5c8f3e5b4ae1a3996d2f6489c3e08eb5aea7 tbase
  0000000000000000000000000000000000000000 tbase
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t1
  0000000000000000000000000000000000000000 t1
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
  0000000000000000000000000000000000000000 t2
  875517b4806a848f942811a315a5bce30804ae85 t5
  0000000000000000000000000000000000000000 t5
  4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
  79505d5360b07e3e79d1052e347e73c02b8afa5b t3
  0000000000000000000000000000000000000000 t3
  ea918d56be86a4afc5a95312e8b6750e1428d9d2 t7
  0000000000000000000000000000000000000000 t7
  ea918d56be86a4afc5a95312e8b6750e1428d9d2 t7
  fd3a9e394ce3afb354a496323bf68ac1755a30de t7

also check that we minimize the diff with the 1st merge parent

  $ hg diff --git -r 'p1()' .hgtags
  diff --git a/.hgtags b/.hgtags
  --- a/.hgtags
  +++ b/.hgtags
  @@ -1,12 +1,17 @@
   6cee5c8f3e5b4ae1a3996d2f6489c3e08eb5aea7 tbase
  +0000000000000000000000000000000000000000 tbase
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t1
  +0000000000000000000000000000000000000000 t1
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t2
   0000000000000000000000000000000000000000 t2
   875517b4806a848f942811a315a5bce30804ae85 t5
  +0000000000000000000000000000000000000000 t5
   4f3e9b90005b68b4d8a3f4355cedc302a8364f5c t3
   79505d5360b07e3e79d1052e347e73c02b8afa5b t3
  +0000000000000000000000000000000000000000 t3
   ea918d56be86a4afc5a95312e8b6750e1428d9d2 t7
  +0000000000000000000000000000000000000000 t7
   ea918d56be86a4afc5a95312e8b6750e1428d9d2 t7
   fd3a9e394ce3afb354a496323bf68ac1755a30de t7

