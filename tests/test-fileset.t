  $ fileset() {
  >   hg debugfileset "$@"
  > }

  $ hg init repo
  $ cd repo
  $ echo a > a1
  $ echo a > a2
  $ echo b > b1
  $ echo b > b2
  $ hg ci -Am addfiles
  adding a1
  adding a2
  adding b1
  adding b2

Test operators and basic patterns

  $ fileset -v a1
  ('symbol', 'a1')
  a1
  $ fileset -v 'a*'
  ('symbol', 'a*')
  a1
  a2
  $ fileset -v '"re:a\d"'
  ('string', 're:a\\d')
  a1
  a2
  $ fileset -v 'a1 or a2'
  (or
    ('symbol', 'a1')
    ('symbol', 'a2'))
  a1
  a2
  $ fileset 'a1 | a2'
  a1
  a2
  $ fileset 'a* and "*1"'
  a1
  $ fileset 'a* & "*1"'
  a1
  $ fileset 'not (r"a*")'
  b1
  b2
  $ fileset '! ("a*")'
  b1
  b2
  $ fileset 'a* - a1'
  a2
  $ fileset 'a_b'

Test files status

  $ rm a1
  $ hg rm a2
  $ echo b >> b2
  $ hg cp b1 c1
  $ echo c > c2
  $ echo c > c3
  $ cat > .hgignore <<EOF
  > \.hgignore
  > 2$
  > EOF
  $ fileset 'modified()'
  b2
  $ fileset 'added()'
  c1
  $ fileset 'removed()'
  a2
  $ fileset 'deleted()'
  a1
  $ fileset 'unknown()'
  c3
  $ fileset 'ignored()'
  .hgignore
  c2
  $ fileset 'hgignore()'
  a2
  b2
  $ fileset 'clean()'
  b1
  $ fileset 'copied()'
  c1

Test files properties

  >>> file('bin', 'wb').write('\0a')
  $ fileset 'binary()'
  $ fileset 'binary() and unknown()'
  bin
  $ echo '^bin$' >> .hgignore
  $ fileset 'binary() and ignored()'
  bin
  $ hg add bin
  $ fileset 'binary()'
  bin

  $ fileset 'grep("b{1}")'
  b2
  c1
  b1
  $ fileset 'grep("missingparens(")'
  hg: parse error: invalid match pattern: unbalanced parenthesis
  [255]

#if execbit
  $ chmod +x b2
  $ fileset 'exec()'
  b2
#endif

#if symlink
  $ ln -s b2 b2link
  $ fileset 'symlink() and unknown()'
  b2link
  $ hg add b2link
#endif

#if no-windows
  $ echo foo > con.xml
  $ fileset 'not portable()'
  con.xml
  $ hg --config ui.portablefilenames=ignore add con.xml
#endif

  >>> file('1k', 'wb').write(' '*1024)
  >>> file('2k', 'wb').write(' '*2048)
  $ hg add 1k 2k
  $ fileset 'size("bar")'
  hg: parse error: couldn't parse size: bar
  [255]
  $ fileset 'size(1k)'
  1k
  $ fileset '(1k or 2k) and size("< 2k")'
  1k
  $ fileset '(1k or 2k) and size("<=2k")'
  1k
  2k
  $ fileset '(1k or 2k) and size("> 1k")'
  2k
  $ fileset '(1k or 2k) and size(">=1K")'
  1k
  2k
  $ fileset '(1k or 2k) and size(".5KB - 1.5kB")'
  1k

Test merge states

  $ hg ci -m manychanges
  $ hg up -C 0
  * files updated, 0 files merged, * files removed, 0 files unresolved (glob)
  $ echo c >> b2
  $ hg ci -m diverging b2
  created new head
  $ fileset 'resolved()'
  $ fileset 'unresolved()'
  $ hg merge
  merging b2
  warning: conflicts during merge.
  merging b2 incomplete! (edit conflicts, then use 'hg resolve --mark')
  * files updated, 0 files merged, * files removed, 1 files unresolved (glob)
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ fileset 'resolved()'
  $ fileset 'unresolved()'
  b2
  $ echo e > b2
  $ hg resolve -m b2
  (no more unresolved files)
  $ fileset 'resolved()'
  b2
  $ fileset 'unresolved()'
  $ hg ci -m merge

Test subrepo predicate

  $ hg init sub
  $ echo a > sub/suba
  $ hg -R sub add sub/suba
  $ hg -R sub ci -m sub
  $ echo 'sub = sub' > .hgsub
  $ hg init sub2
  $ echo b > sub2/b
  $ hg -R sub2 ci -Am sub2
  adding b
  $ echo 'sub2 = sub2' >> .hgsub
  $ fileset 'subrepo()'
  $ hg add .hgsub
  $ fileset 'subrepo()'
  sub
  sub2
  $ fileset 'subrepo("sub")'
  sub
  $ fileset 'subrepo("glob:*")'
  sub
  sub2
  $ hg ci -m subrepo

Test that .hgsubstate is updated as appropriate during a conversion.  The
saverev property is enough to alter the hashes of the subrepo.

  $ hg init ../converted
  $ hg --config extensions.convert= convert --config convert.hg.saverev=True  \
  >      sub ../converted/sub
  initializing destination ../converted/sub repository
  scanning source...
  sorting...
  converting...
  0 sub
  $ hg clone -U sub2 ../converted/sub2
  $ hg --config extensions.convert= convert --config convert.hg.saverev=True  \
  >      . ../converted
  scanning source...
  sorting...
  converting...
  4 addfiles
  3 manychanges
  2 diverging
  1 merge
  0 subrepo
  no ".hgsubstate" updates will be made for "sub2"
  $ hg up -q -R ../converted -r tip
  $ hg --cwd ../converted cat sub/suba sub2/b -r tip
  a
  b
  $ oldnode=`hg log -r tip -T "{node}\n"`
  $ newnode=`hg log -R ../converted -r tip -T "{node}\n"`
  $ [ "$oldnode" != "$newnode" ] || echo "nothing changed"

Test with a revision

  $ hg log -G --template '{rev} {desc}\n'
  @  4 subrepo
  |
  o    3 merge
  |\
  | o  2 diverging
  | |
  o |  1 manychanges
  |/
  o  0 addfiles
  
  $ echo unknown > unknown
  $ fileset -r1 'modified()'
  b2
  $ fileset -r1 'added() and c1'
  c1
  $ fileset -r1 'removed()'
  a2
  $ fileset -r1 'deleted()'
  $ fileset -r1 'unknown()'
  $ fileset -r1 'ignored()'
  $ fileset -r1 'hgignore()'
  b2
  bin
  $ fileset -r1 'binary()'
  bin
  $ fileset -r1 'size(1k)'
  1k
  $ fileset -r3 'resolved()'
  $ fileset -r3 'unresolved()'

#if execbit
  $ fileset -r1 'exec()'
  b2
#endif

#if symlink
  $ fileset -r1 'symlink()'
  b2link
#endif

#if no-windows
  $ fileset -r1 'not portable()'
  con.xml
  $ hg forget 'con.xml'
#endif

  $ fileset -r4 'subrepo("re:su.*")'
  sub
  sub2
  $ fileset -r4 'subrepo("sub")'
  sub
  $ fileset -r4 'b2 or c1'
  b2
  c1

  >>> open('dos', 'wb').write("dos\r\n")
  >>> open('mixed', 'wb').write("dos\r\nunix\n")
  >>> open('mac', 'wb').write("mac\r")
  $ hg add dos mixed mac

  $ fileset 'eol(dos)'
  dos
  mixed
  $ fileset 'eol(unix)'
  .hgsub
  .hgsubstate
  a1
  b1
  b2
  c1
  mixed
  $ fileset 'eol(mac)'
  mac
