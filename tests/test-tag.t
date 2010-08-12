  $ hg init test
  $ cd test

  $ echo a > a
  $ hg add a
  $ hg commit -m "test" -d "1000000 0"
  $ hg history
  changeset:   0:0acdaf898367
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     test
  

  $ hg tag ' '
  abort: tag names cannot consist entirely of whitespace

  $ hg tag -d "1000000 0" "bleah"
  $ hg history
  changeset:   1:3ecf002a1c57
  tag:         tip
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Added tag bleah for changeset 0acdaf898367
  
  changeset:   0:0acdaf898367
  tag:         bleah
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     test
  

  $ echo foo >> .hgtags
  $ hg tag -d "1000000 0" "bleah2" || echo "failed"
  abort: working copy of .hgtags is changed (please commit .hgtags manually)
  failed

  $ hg revert .hgtags
  $ hg tag -d "1000000 0" -r 0 x y z y y z || echo "failed"
  abort: tag names must be unique
  failed
  $ hg tag -d "1000000 0" tap nada dot tip null . || echo "failed"
  abort: the name 'tip' is reserved
  failed
  $ hg tag -d "1000000 0" "bleah" || echo "failed"
  abort: tag 'bleah' already exists (use -f to force)
  failed
  $ hg tag -d "1000000 0" "blecch" "bleah" || echo "failed"
  abort: tag 'bleah' already exists (use -f to force)
  failed

  $ hg tag -d "1000000 0" --remove "blecch" || echo "failed"
  abort: tag 'blecch' does not exist
  failed
  $ hg tag -d "1000000 0" --remove "bleah" "blecch" "blough" || echo "failed"
  abort: tag 'blecch' does not exist
  failed

  $ hg tag -d "1000000 0" -r 0 "bleah0"
  $ hg tag -l -d "1000000 0" -r 1 "bleah1"
  $ hg tag -d "1000000 0" gack gawk gorp
  $ hg tag -d "1000000 0" -f gack
  $ hg tag -d "1000000 0" --remove gack gorp

  $ cat .hgtags
  0acdaf8983679e0aac16e811534eb49d7ee1f2b4 bleah
  0acdaf8983679e0aac16e811534eb49d7ee1f2b4 bleah0
  868cc8fbb43b754ad09fa109885d243fc49adae7 gack
  868cc8fbb43b754ad09fa109885d243fc49adae7 gawk
  868cc8fbb43b754ad09fa109885d243fc49adae7 gorp
  868cc8fbb43b754ad09fa109885d243fc49adae7 gack
  3807bcf62c5614cb6c16436b514d7764ca5f1631 gack
  3807bcf62c5614cb6c16436b514d7764ca5f1631 gack
  0000000000000000000000000000000000000000 gack
  868cc8fbb43b754ad09fa109885d243fc49adae7 gorp
  0000000000000000000000000000000000000000 gorp
  $ cat .hg/localtags
  3ecf002a1c572a2f3bb4e665417e60fca65bbd42 bleah1

  $ hg update 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg tag -d "1000000 0" "foobar"
  $ cat .hgtags
  0acdaf8983679e0aac16e811534eb49d7ee1f2b4 foobar
  $ cat .hg/localtags
  3ecf002a1c572a2f3bb4e665417e60fca65bbd42 bleah1

  $ hg tag -l 'xx
  > newline'
  abort: '\n' cannot be used in a tag name
  $ hg tag -l 'xx:xx'
  abort: ':' cannot be used in a tag name

cloning local tags

  $ cd ..
  $ hg -R test log -r0:5
  changeset:   0:0acdaf898367
  tag:         bleah
  tag:         bleah0
  tag:         foobar
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     test
  
  changeset:   1:3ecf002a1c57
  tag:         bleah1
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Added tag bleah for changeset 0acdaf898367
  
  changeset:   2:868cc8fbb43b
  tag:         gawk
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Added tag bleah0 for changeset 0acdaf898367
  
  changeset:   3:3807bcf62c56
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Added tag gack, gawk, gorp for changeset 868cc8fbb43b
  
  changeset:   4:140c6e8597b4
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Added tag gack for changeset 3807bcf62c56
  
  changeset:   5:470a65fa7cc9
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Removed tag gack, gorp
  
  $ hg clone -q -rbleah1 test test1
  $ hg -R test1 parents --style=compact
  1[tip]   3ecf002a1c57   1970-01-12 13:46 +0000   test
    Added tag bleah for changeset 0acdaf898367
  
  $ hg clone -q -r5 test#bleah1 test2
  $ hg -R test2 parents --style=compact
  5[tip]   470a65fa7cc9   1970-01-12 13:46 +0000   test
    Removed tag gack, gorp
  
  $ hg clone -q -U test#bleah1 test3
  $ hg -R test3 parents --style=compact

  $ cd test

issue 601

  $ python << EOF
  > f = file('.hg/localtags'); last = f.readlines()[-1][:-1]; f.close()
  > f = file('.hg/localtags', 'w'); f.write(last); f.close()
  > EOF
  $ cat .hg/localtags; echo
  3ecf002a1c572a2f3bb4e665417e60fca65bbd42 bleah1
  $ hg tag -l localnewline
  $ cat .hg/localtags; echo
  3ecf002a1c572a2f3bb4e665417e60fca65bbd42 bleah1
  f68b039e72eacbb2e68b0543e1f6e50990aa2bb5 localnewline
  

  $ python << EOF
  > f = file('.hgtags'); last = f.readlines()[-1][:-1]; f.close()
  > f = file('.hgtags', 'w'); f.write(last); f.close()
  > EOF
  $ hg ci -d '1000000 0' -m'broken manual edit of .hgtags'
  $ cat .hgtags; echo
  0acdaf8983679e0aac16e811534eb49d7ee1f2b4 foobar
  $ hg tag -d '1000000 0' newline
  $ cat .hgtags; echo
  0acdaf8983679e0aac16e811534eb49d7ee1f2b4 foobar
  6ae703d793c8b1f097116869275ecd97b2977a2b newline
  

tag and branch using same name

  $ hg branch tag-and-branch-same-name
  marked working directory as branch tag-and-branch-same-name
  $ hg ci -m"discouraged"
  $ hg tag tag-and-branch-same-name
  warning: tag tag-and-branch-same-name conflicts with existing branch name

test custom commit messages

  $ cat > $HGTMP/editor <<'__EOF__'
  > #!/bin/sh
  > echo "custom tag message" > "$1"
  > echo "second line" >> "$1"
  > __EOF__
  $ chmod +x "$HGTMP"/editor
  $ HGEDITOR="'$HGTMP'"/editor hg tag custom-tag -e
  $ hg log -l1 --template "{desc}\n"
  custom tag message
  second line
