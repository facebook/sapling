  $ . helpers-usechg.sh

  $ setconfig ui.allowemptycommit=1

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg add a
  $ echo line 1 > b
  $ echo line 2 >> b
  $ hg commit -l b -d '1000000 0' -u 'User Name <user@hostname>'

  $ hg add b
  $ echo other 1 > c
  $ echo other 2 >> c
  $ echo >> c
  $ echo other 3 >> c
  $ hg commit -l c -d '1100000 0' -u 'A. N. Other <other@place>'

  $ hg add c
  $ hg commit -m 'no person' -d '1200000 0' -u 'other@place'
  $ echo c >> c
  $ hg commit -m 'no user, no domain' -d '1300000 0' -u 'person'

  $ hg commit -m 'new branch' -d '1400000 0' -u 'person'
  $ hg bookmark foo

  $ hg co -q 3
  $ echo other 4 >> d
  $ hg add d
  $ hg commit -m 'new head' -d '1500000 0' -u 'person'

  $ hg merge -q foo
  $ hg commit -m 'merge' -d '1500001 0' -u 'person'

  $ hg log -r . -T '{username}'
  test (no-eol)

Test arithmetic operators have the right precedence:

  $ hg log -l 1 -T '{date(date, "%Y") + 5 * 10} {date(date, "%Y") - 2 * 3}\n'
  2020 1964
  $ hg log -l 1 -T '{date(date, "%Y") * 5 + 10} {date(date, "%Y") * 3 - 2}\n'
  9860 5908

Test division:

  $ hg debugtemplate -r0 -v '{5 / 2} {mod(5, 2)}\n'
  (template
    (/
      (integer '5')
      (integer '2'))
    (string ' ')
    (func
      (symbol 'mod')
      (list
        (integer '5')
        (integer '2')))
    (string '\n'))
  2 1
  $ hg debugtemplate -r0 -v '{5 / -2} {mod(5, -2)}\n'
  (template
    (/
      (integer '5')
      (negate
        (integer '2')))
    (string ' ')
    (func
      (symbol 'mod')
      (list
        (integer '5')
        (negate
          (integer '2'))))
    (string '\n'))
  -3 -1
  $ hg debugtemplate -r0 -v '{-5 / 2} {mod(-5, 2)}\n'
  (template
    (/
      (negate
        (integer '5'))
      (integer '2'))
    (string ' ')
    (func
      (symbol 'mod')
      (list
        (negate
          (integer '5'))
        (integer '2')))
    (string '\n'))
  -3 1
  $ hg debugtemplate -r0 -v '{-5 / -2} {mod(-5, -2)}\n'
  (template
    (/
      (negate
        (integer '5'))
      (negate
        (integer '2')))
    (string ' ')
    (func
      (symbol 'mod')
      (list
        (negate
          (integer '5'))
        (negate
          (integer '2'))))
    (string '\n'))
  2 -1

Filters bind closer than arithmetic:

  $ hg debugtemplate -r0 -v '{revset(".")|count - 1}\n'
  (template
    (-
      (|
        (func
          (symbol 'revset')
          (string '.'))
        (symbol 'count'))
      (integer '1'))
    (string '\n'))
  0

But negate binds closer still:

  $ hg debugtemplate -r0 -v '{1-3|stringify}\n'
  (template
    (-
      (integer '1')
      (|
        (integer '3')
        (symbol 'stringify')))
    (string '\n'))
  hg: parse error: arithmetic only defined on integers
  [255]
  $ hg debugtemplate -r0 -v '{-3|stringify}\n'
  (template
    (|
      (negate
        (integer '3'))
      (symbol 'stringify'))
    (string '\n'))
  -3

Filters bind as close as map operator:

  $ hg debugtemplate -r0 -v '{desc|splitlines % "{line}\n"}'
  (template
    (%
      (|
        (symbol 'desc')
        (symbol 'splitlines'))
      (template
        (symbol 'line')
        (string '\n'))))
  line 1
  line 2

Keyword arguments:

  $ hg debugtemplate -r0 -v '{foo=bar|baz}'
  (template
    (keyvalue
      (symbol 'foo')
      (|
        (symbol 'bar')
        (symbol 'baz'))))
  hg: parse error: can't use a key-value pair in this context
  [255]

  $ hg debugtemplate '{pad("foo", width=10, left=true)}\n'
         foo

Call function which takes named arguments by filter syntax:

  $ hg debugtemplate '{" "|separate}'
  $ hg debugtemplate '{("not", "an", "argument", "list")|separate}'
  hg: parse error: unknown method 'list'
  [255]

Second branch starting at nullrev:

  $ hg update null
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ echo second > second
  $ hg add second
  $ hg commit -m second -d '1000000 0' -u 'User Name <user@hostname>'

  $ echo third > third
  $ hg add third
  $ hg mv second fourth
  $ hg commit -m third -d "2020-01-01 10:01"

  $ hg log --template '{join(file_copies, ",\n")}\n' -r .
  fourth (second)
  $ hg log -T '{file_copies % "{source} -> {name}\n"}' -r .
  second -> fourth
  $ hg log -T '{rev} {ifcontains("fourth", file_copies, "t", "f")}\n' -r .:7
  8 t
  7 f

Working-directory revision has special identifiers, though they are still
experimental:

  $ hg log -r 'wdir()' -T '{rev}:{node}\n'
  2147483647:ffffffffffffffffffffffffffffffffffffffff

Some keywords are invalid for working-directory revision, but they should
never cause crash:

  $ hg log -r 'wdir()' -T '{manifest}\n'
  

Quoting for ui.logtemplate

  $ hg tip --config "ui.logtemplate={rev}\n"
  8
  $ hg tip --config "ui.logtemplate='{rev}\n'"
  8
  $ hg tip --config 'ui.logtemplate="{rev}\n"'
  8
  $ hg tip --config 'ui.logtemplate=n{rev}\n'
  n8

Make sure user/global hgrc does not affect tests

  $ echo '[ui]' > .hg/hgrc
  $ echo 'logtemplate =' >> .hg/hgrc
  $ echo 'style =' >> .hg/hgrc

Add some simple styles to settings

  $ cat <<'EOF' >> .hg/hgrc
  > [templates]
  > simple = "{rev}\n"
  > simple2 = {rev}\n
  > rev = "should not precede {rev} keyword\n"
  > EOF

  $ hg log -l1 -Tsimple
  8
  $ hg log -l1 -Tsimple2
  8
  $ hg log -l1 -Trev
  should not precede 8 keyword
  $ hg log -l1 -T '{simple}'
  8

Map file shouldn't see user templates:

  $ cat <<EOF > tmpl
  > changeset = 'nothing expanded:{simple}\n'
  > EOF
  $ hg log -l1 --style ./tmpl
  nothing expanded:

Test templates and style maps in files:

  $ echo "{rev}" > tmpl
  $ hg log -l1 -T./tmpl
  8
  $ hg log -l1 -Tblah/blah
  blah/blah (no-eol)

  $ printf 'changeset = "{rev}\\n"\n' > map-simple
  $ hg log -l1 -T./map-simple
  8

 a map file may have [templates] and [templatealias] sections:

  $ cat <<'EOF' > map-simple
  > [templates]
  > changeset = "{a}\n"
  > [templatealias]
  > a = rev
  > EOF
  $ hg log -l1 -T./map-simple
  8

 so it can be included in hgrc

  $ cat <<'EOF' > myhgrc
  > %include map-simple
  > [templates]
  > foo = "{changeset}"
  > EOF
  $ HGRCPATH=./myhgrc hg log -l1 -Tfoo
  8
  $ HGRCPATH=./myhgrc hg log -l1 -T'{a}\n'
  8

Test template map inheritance

  $ echo "__base__ = map-cmdline.default" > map-simple
  $ printf 'cset = "changeset: ***{rev}***\\n"\n' >> map-simple
  $ hg log -l1 -T./map-simple
  changeset: ***8***
  tag:         tip
  user:        test
  date:        Wed Jan 01 10:01:00 2020 +0000
  summary:     third
  

Test docheader, docfooter and separator in template map

  $ cat <<'EOF' > map-myjson
  > docheader = '\{\n'
  > docfooter = '\n}\n'
  > separator = ',\n'
  > changeset = ' {dict(rev, node|short)|json}'
  > EOF
  $ hg log -l2 -T./map-myjson
  {
   {"node": "95c24699272e", "rev": 8},
   {"node": "29114dbae42b", "rev": 7}
  }

Test docheader, docfooter and separator in [templates] section

  $ cat <<'EOF' >> .hg/hgrc
  > [templates]
  > myjson = ' {dict(rev, node|short)|json}'
  > myjson:docheader = '\{\n'
  > myjson:docfooter = '\n}\n'
  > myjson:separator = ',\n'
  > :docheader = 'should not be selected as a docheader for literal templates\n'
  > EOF
  $ hg log -l2 -Tmyjson
  {
   {"node": "95c24699272e", "rev": 8},
   {"node": "29114dbae42b", "rev": 7}
  }
  $ hg log -l1 -T'{rev}\n'
  8

Template should precede style option

  $ hg log -l1 --style default -T '{rev}\n'
  8

Add a commit with empty description, to ensure that the templates
below will omit the description line.

  $ echo c >> c
  $ hg add c
  $ hg commit -qm ' '

Default style is like normal output. Phases style should be the same
as default style, except for extra phase lines.

  $ hg log > log.out
  $ hg log --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg log -T phases > phases.out
  $ diff -U 0 log.out phases.out | egrep -v '^---|^\+\+\+|^@@'
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft

  $ hg log -v > log.out
  $ hg log -v --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg log -v -T phases > phases.out
  $ diff -U 0 log.out phases.out | egrep -v '^---|^\+\+\+|^@@'
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft
  +phase:       draft

  $ hg log -q > log.out
  $ hg log -q --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg log -q -T phases > phases.out
  $ cmp log.out phases.out || diff -u log.out phases.out

  $ hg log --debug > log.out
  $ hg log --debug --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg log --debug -T phases > phases.out
  $ cmp log.out phases.out || diff -u log.out phases.out

Default style of working-directory revision should also be the same (but
date may change while running tests):

  $ hg log -r 'wdir()' | sed 's|^date:.*|date:|' > log.out
  $ hg log -r 'wdir()' --style default | sed 's|^date:.*|date:|' > style.out
  $ cmp log.out style.out || diff -u log.out style.out

  $ hg log -r 'wdir()' -v | sed 's|^date:.*|date:|' > log.out
  $ hg log -r 'wdir()' -v --style default | sed 's|^date:.*|date:|' > style.out
  $ cmp log.out style.out || diff -u log.out style.out

  $ hg log -r 'wdir()' -q > log.out
  $ hg log -r 'wdir()' -q --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out

  $ hg log -r 'wdir()' --debug | sed 's|^date:.*|date:|' > log.out
  $ hg log -r 'wdir()' --debug --style default \
  > | sed 's|^date:.*|date:|' > style.out
  $ cmp log.out style.out || diff -u log.out style.out

Default style should also preserve color information (issue2866):

  $ cp $HGRCPATH $HGRCPATH-bak
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > color=
  > EOF

  $ hg --color=debug log > log.out
  $ hg --color=debug log --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg --color=debug log -T phases > phases.out
  $ diff -U 0 log.out phases.out | egrep -v '^---|^\+\+\+|^@@'
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]

  $ hg --color=debug -v log > log.out
  $ hg --color=debug -v log --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg --color=debug -v log -T phases > phases.out
  $ diff -U 0 log.out phases.out | egrep -v '^---|^\+\+\+|^@@'
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]
  +[log.phase|phase:       draft]

  $ hg --color=debug -q log > log.out
  $ hg --color=debug -q log --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg --color=debug -q log -T phases > phases.out
  $ cmp log.out phases.out || diff -u log.out phases.out

  $ hg --color=debug --debug log > log.out
  $ hg --color=debug --debug log --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg --color=debug --debug log -T phases > phases.out
  $ cmp log.out phases.out || diff -u log.out phases.out

  $ mv $HGRCPATH-bak $HGRCPATH

Remove commit with empty commit message, so as to not pollute further
tests.

  $ hg debugstrip -q .

Revision with no copies (used to print a traceback):

  $ hg tip -v --template '\n'
  

Compact style works:

  $ hg log -Tcompact
  8[tip]   95c24699272e   2020-01-01 10:01 +0000   test
    third
  
  7:-1   29114dbae42b   1970-01-12 13:46 +0000   user
    second
  
  6:5,4   f7e5795620e7   1970-01-18 08:40 +0000   person
    merge
  
  5:3   13207e5a10d9   1970-01-18 08:40 +0000   person
    new head
  
  4[foo]   07fa1db10648   1970-01-17 04:53 +0000   person
    new branch
  
  3   10e46f2dcbf4   1970-01-16 01:06 +0000   person
    no user, no domain
  
  2   97054abb4ab8   1970-01-14 21:20 +0000   other
    no person
  
  1   b608e9d1a3f0   1970-01-13 17:33 +0000   other
    other 1
  
  0   1e4e1b8f71e0   1970-01-12 13:46 +0000   user
    line 1
  

  $ hg log -v --style compact
  8[tip]   95c24699272e   2020-01-01 10:01 +0000   test
    third
  
  7:-1   29114dbae42b   1970-01-12 13:46 +0000   User Name <user@hostname>
    second
  
  6:5,4   f7e5795620e7   1970-01-18 08:40 +0000   person
    merge
  
  5:3   13207e5a10d9   1970-01-18 08:40 +0000   person
    new head
  
  4   07fa1db10648   1970-01-17 04:53 +0000   person
    new branch
  
  3   10e46f2dcbf4   1970-01-16 01:06 +0000   person
    no user, no domain
  
  2   97054abb4ab8   1970-01-14 21:20 +0000   other@place
    no person
  
  1   b608e9d1a3f0   1970-01-13 17:33 +0000   A. N. Other <other@place>
    other 1
  other 2
  
  other 3
  
  0   1e4e1b8f71e0   1970-01-12 13:46 +0000   User Name <user@hostname>
    line 1
  line 2
  

  $ hg log --debug --style compact
  8[tip]:7,-1   95c24699272e   2020-01-01 10:01 +0000   test
    third
  
  7:-1,-1   29114dbae42b   1970-01-12 13:46 +0000   User Name <user@hostname>
    second
  
  6:5,4   f7e5795620e7   1970-01-18 08:40 +0000   person
    merge
  
  5:3,-1   13207e5a10d9   1970-01-18 08:40 +0000   person
    new head
  
  4:3,-1   07fa1db10648   1970-01-17 04:53 +0000   person
    new branch
  
  3:2,-1   10e46f2dcbf4   1970-01-16 01:06 +0000   person
    no user, no domain
  
  2:1,-1   97054abb4ab8   1970-01-14 21:20 +0000   other@place
    no person
  
  1:0,-1   b608e9d1a3f0   1970-01-13 17:33 +0000   A. N. Other <other@place>
    other 1
  other 2
  
  other 3
  
  0:-1,-1   1e4e1b8f71e0   1970-01-12 13:46 +0000   User Name <user@hostname>
    line 1
  line 2
  

Test xml styles:

  $ hg log --style xml -r 'not all()'
  <?xml version="1.0"?>
  <log>
  </log>

  $ hg log --style xml
  <?xml version="1.0"?>
  <log>
  <logentry revision="8" node="95c24699272ef57d062b8bccc32c878bf841784a">
  <tag>tip</tag>
  <author email="test">test</author>
  <date>2020-01-01T10:01:00+00:00</date>
  <msg xml:space="preserve">third</msg>
  </logentry>
  <logentry revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453">
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="user@hostname">User Name</author>
  <date>1970-01-12T13:46:40+00:00</date>
  <msg xml:space="preserve">second</msg>
  </logentry>
  <logentry revision="6" node="f7e5795620e78993ad76680c4306bb2da83907b3">
  <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
  <parent revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce" />
  <author email="person">person</author>
  <date>1970-01-18T08:40:01+00:00</date>
  <msg xml:space="preserve">merge</msg>
  </logentry>
  <logentry revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f">
  <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
  <author email="person">person</author>
  <date>1970-01-18T08:40:00+00:00</date>
  <msg xml:space="preserve">new head</msg>
  </logentry>
  <logentry revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce">
  <bookmark>foo</bookmark>
  <author email="person">person</author>
  <date>1970-01-17T04:53:20+00:00</date>
  <msg xml:space="preserve">new branch</msg>
  </logentry>
  <logentry revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47">
  <author email="person">person</author>
  <date>1970-01-16T01:06:40+00:00</date>
  <msg xml:space="preserve">no user, no domain</msg>
  </logentry>
  <logentry revision="2" node="97054abb4ab824450e9164180baf491ae0078465">
  <author email="other@place">other</author>
  <date>1970-01-14T21:20:00+00:00</date>
  <msg xml:space="preserve">no person</msg>
  </logentry>
  <logentry revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965">
  <author email="other@place">A. N. Other</author>
  <date>1970-01-13T17:33:20+00:00</date>
  <msg xml:space="preserve">other 1
  other 2
  
  other 3</msg>
  </logentry>
  <logentry revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f">
  <author email="user@hostname">User Name</author>
  <date>1970-01-12T13:46:40+00:00</date>
  <msg xml:space="preserve">line 1
  line 2</msg>
  </logentry>
  </log>

  $ hg log -v --style xml
  <?xml version="1.0"?>
  <log>
  <logentry revision="8" node="95c24699272ef57d062b8bccc32c878bf841784a">
  <tag>tip</tag>
  <author email="test">test</author>
  <date>2020-01-01T10:01:00+00:00</date>
  <msg xml:space="preserve">third</msg>
  <paths>
  <path action="A">fourth</path>
  <path action="A">third</path>
  <path action="R">second</path>
  </paths>
  <copies>
  <copy source="second">fourth</copy>
  </copies>
  </logentry>
  <logentry revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453">
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="user@hostname">User Name</author>
  <date>1970-01-12T13:46:40+00:00</date>
  <msg xml:space="preserve">second</msg>
  <paths>
  <path action="A">second</path>
  </paths>
  </logentry>
  <logentry revision="6" node="f7e5795620e78993ad76680c4306bb2da83907b3">
  <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
  <parent revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce" />
  <author email="person">person</author>
  <date>1970-01-18T08:40:01+00:00</date>
  <msg xml:space="preserve">merge</msg>
  <paths>
  </paths>
  </logentry>
  <logentry revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f">
  <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
  <author email="person">person</author>
  <date>1970-01-18T08:40:00+00:00</date>
  <msg xml:space="preserve">new head</msg>
  <paths>
  <path action="A">d</path>
  </paths>
  </logentry>
  <logentry revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce">
  <bookmark>foo</bookmark>
  <author email="person">person</author>
  <date>1970-01-17T04:53:20+00:00</date>
  <msg xml:space="preserve">new branch</msg>
  <paths>
  </paths>
  </logentry>
  <logentry revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47">
  <author email="person">person</author>
  <date>1970-01-16T01:06:40+00:00</date>
  <msg xml:space="preserve">no user, no domain</msg>
  <paths>
  <path action="M">c</path>
  </paths>
  </logentry>
  <logentry revision="2" node="97054abb4ab824450e9164180baf491ae0078465">
  <author email="other@place">other</author>
  <date>1970-01-14T21:20:00+00:00</date>
  <msg xml:space="preserve">no person</msg>
  <paths>
  <path action="A">c</path>
  </paths>
  </logentry>
  <logentry revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965">
  <author email="other@place">A. N. Other</author>
  <date>1970-01-13T17:33:20+00:00</date>
  <msg xml:space="preserve">other 1
  other 2
  
  other 3</msg>
  <paths>
  <path action="A">b</path>
  </paths>
  </logentry>
  <logentry revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f">
  <author email="user@hostname">User Name</author>
  <date>1970-01-12T13:46:40+00:00</date>
  <msg xml:space="preserve">line 1
  line 2</msg>
  <paths>
  <path action="A">a</path>
  </paths>
  </logentry>
  </log>

  $ hg log --debug --style xml
  <?xml version="1.0"?>
  <log>
  <logentry revision="8" node="95c24699272ef57d062b8bccc32c878bf841784a">
  <tag>tip</tag>
  <parent revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="test">test</author>
  <date>2020-01-01T10:01:00+00:00</date>
  <msg xml:space="preserve">third</msg>
  <paths>
  <path action="A">fourth</path>
  <path action="A">third</path>
  <path action="R">second</path>
  </paths>
  <copies>
  <copy source="second">fourth</copy>
  </copies>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="7" node="29114dbae42b9f078cf2714dbe3a86bba8ec7453">
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="user@hostname">User Name</author>
  <date>1970-01-12T13:46:40+00:00</date>
  <msg xml:space="preserve">second</msg>
  <paths>
  <path action="A">second</path>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="6" node="f7e5795620e78993ad76680c4306bb2da83907b3">
  <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
  <parent revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce" />
  <author email="person">person</author>
  <date>1970-01-18T08:40:01+00:00</date>
  <msg xml:space="preserve">merge</msg>
  <paths>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f">
  <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="person">person</author>
  <date>1970-01-18T08:40:00+00:00</date>
  <msg xml:space="preserve">new head</msg>
  <paths>
  <path action="A">d</path>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="4" node="07fa1db1064879a32157227401eb44b322ae53ce">
  <bookmark>foo</bookmark>
  <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="person">person</author>
  <date>1970-01-17T04:53:20+00:00</date>
  <msg xml:space="preserve">new branch</msg>
  <paths>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47">
  <parent revision="2" node="97054abb4ab824450e9164180baf491ae0078465" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="person">person</author>
  <date>1970-01-16T01:06:40+00:00</date>
  <msg xml:space="preserve">no user, no domain</msg>
  <paths>
  <path action="M">c</path>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="2" node="97054abb4ab824450e9164180baf491ae0078465">
  <parent revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="other@place">other</author>
  <date>1970-01-14T21:20:00+00:00</date>
  <msg xml:space="preserve">no person</msg>
  <paths>
  <path action="A">c</path>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="1" node="b608e9d1a3f0273ccf70fb85fd6866b3482bf965">
  <parent revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="other@place">A. N. Other</author>
  <date>1970-01-13T17:33:20+00:00</date>
  <msg xml:space="preserve">other 1
  other 2
  
  other 3</msg>
  <paths>
  <path action="A">b</path>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  <logentry revision="0" node="1e4e1b8f71e05681d422154f5421e385fec3454f">
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="user@hostname">User Name</author>
  <date>1970-01-12T13:46:40+00:00</date>
  <msg xml:space="preserve">line 1
  line 2</msg>
  <paths>
  <path action="A">a</path>
  </paths>
  <extra key="branch">default</extra>
  </logentry>
  </log>


Test JSON style:

  $ hg log -k nosuch -Tjson
  []

  $ hg log -qr . -Tjson
  [
   {
    "rev": 8,
    "node": "95c24699272ef57d062b8bccc32c878bf841784a"
   }
  ]

  $ hg log -vpr . -Tjson --stat
  [
   {
    "rev": 8,
    "node": "95c24699272ef57d062b8bccc32c878bf841784a",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [1577872860, 0],
    "desc": "third",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
    "files": ["fourth", "second", "third"],
    "diffstat": " fourth |  1 +\n second |  1 -\n third  |  1 +\n 3 files changed, 2 insertions(+), 1 deletions(-)\n",
    "diff": "diff -r 29114dbae42b -r 95c24699272e fourth\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/fourth\tWed Jan 01 10:01:00 2020 +0000\n@@ -0,0 +1,1 @@\n+second\ndiff -r 29114dbae42b -r 95c24699272e second\n--- a/second\tMon Jan 12 13:46:40 1970 +0000\n+++ /dev/null\tThu Jan 01 00:00:00 1970 +0000\n@@ -1,1 +0,0 @@\n-second\ndiff -r 29114dbae42b -r 95c24699272e third\n--- /dev/null\tThu Jan 01 00:00:00 1970 +0000\n+++ b/third\tWed Jan 01 10:01:00 2020 +0000\n@@ -0,0 +1,1 @@\n+third\n"
   }
  ]

honor --git but not format-breaking diffopts
  $ hg --config diff.noprefix=True log --git -vpr . -Tjson
  [
   {
    "rev": 8,
    "node": "95c24699272ef57d062b8bccc32c878bf841784a",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [1577872860, 0],
    "desc": "third",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
    "files": ["fourth", "second", "third"],
    "diff": "diff --git a/second b/fourth\nrename from second\nrename to fourth\ndiff --git a/third b/third\nnew file mode 100644\n--- /dev/null\n+++ b/third\n@@ -0,0 +1,1 @@\n+third\n"
   }
  ]

  $ hg log -T json
  [
   {
    "rev": 8,
    "node": "95c24699272ef57d062b8bccc32c878bf841784a",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [1577872860, 0],
    "desc": "third",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"]
   },
   {
    "rev": 7,
    "node": "29114dbae42b9f078cf2714dbe3a86bba8ec7453",
    "branch": "default",
    "phase": "draft",
    "user": "User Name <user@hostname>",
    "date": [1000000, 0],
    "desc": "second",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"]
   },
   {
    "rev": 6,
    "node": "f7e5795620e78993ad76680c4306bb2da83907b3",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500001, 0],
    "desc": "merge",
    "bookmarks": [],
    "tags": [],
    "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "07fa1db1064879a32157227401eb44b322ae53ce"]
   },
   {
    "rev": 5,
    "node": "13207e5a10d9fd28ec424934298e176197f2c67f",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500000, 0],
    "desc": "new head",
    "bookmarks": [],
    "tags": [],
    "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"]
   },
   {
    "rev": 4,
    "node": "07fa1db1064879a32157227401eb44b322ae53ce",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1400000, 0],
    "desc": "new branch",
    "bookmarks": ["foo"],
    "tags": [],
    "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"]
   },
   {
    "rev": 3,
    "node": "10e46f2dcbf4823578cf180f33ecf0b957964c47",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1300000, 0],
    "desc": "no user, no domain",
    "bookmarks": [],
    "tags": [],
    "parents": ["97054abb4ab824450e9164180baf491ae0078465"]
   },
   {
    "rev": 2,
    "node": "97054abb4ab824450e9164180baf491ae0078465",
    "branch": "default",
    "phase": "draft",
    "user": "other@place",
    "date": [1200000, 0],
    "desc": "no person",
    "bookmarks": [],
    "tags": [],
    "parents": ["b608e9d1a3f0273ccf70fb85fd6866b3482bf965"]
   },
   {
    "rev": 1,
    "node": "b608e9d1a3f0273ccf70fb85fd6866b3482bf965",
    "branch": "default",
    "phase": "draft",
    "user": "A. N. Other <other@place>",
    "date": [1100000, 0],
    "desc": "other 1\nother 2\n\nother 3",
    "bookmarks": [],
    "tags": [],
    "parents": ["1e4e1b8f71e05681d422154f5421e385fec3454f"]
   },
   {
    "rev": 0,
    "node": "1e4e1b8f71e05681d422154f5421e385fec3454f",
    "branch": "default",
    "phase": "draft",
    "user": "User Name <user@hostname>",
    "date": [1000000, 0],
    "desc": "line 1\nline 2",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"]
   }
  ]

  $ hg heads -v -Tjson
  [
   {
    "rev": 8,
    "node": "95c24699272ef57d062b8bccc32c878bf841784a",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [1577872860, 0],
    "desc": "third",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
    "files": ["fourth", "second", "third"]
   },
   {
    "rev": 6,
    "node": "f7e5795620e78993ad76680c4306bb2da83907b3",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500001, 0],
    "desc": "merge",
    "bookmarks": [],
    "tags": [],
    "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "07fa1db1064879a32157227401eb44b322ae53ce"],
    "files": []
   }
  ]

  $ hg log --debug -Tjson
  [
   {
    "rev": 8,
    "node": "95c24699272ef57d062b8bccc32c878bf841784a",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [1577872860, 0],
    "desc": "third",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["29114dbae42b9f078cf2714dbe3a86bba8ec7453"],
    "manifest": "94961b75a2da554b4df6fb599e5bfc7d48de0c64",
    "extra": {"branch": "default"},
    "modified": [],
    "added": ["fourth", "third"],
    "removed": ["second"]
   },
   {
    "rev": 7,
    "node": "29114dbae42b9f078cf2714dbe3a86bba8ec7453",
    "branch": "default",
    "phase": "draft",
    "user": "User Name <user@hostname>",
    "date": [1000000, 0],
    "desc": "second",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"],
    "manifest": "f2dbc354b94e5ec0b4f10680ee0cee816101d0bf",
    "extra": {"branch": "default"},
    "modified": [],
    "added": ["second"],
    "removed": []
   },
   {
    "rev": 6,
    "node": "f7e5795620e78993ad76680c4306bb2da83907b3",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500001, 0],
    "desc": "merge",
    "bookmarks": [],
    "tags": [],
    "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "07fa1db1064879a32157227401eb44b322ae53ce"],
    "manifest": "4dc3def4f9b4c6e8de820f6ee74737f91e96a216",
    "extra": {"branch": "default"},
    "modified": [],
    "added": [],
    "removed": []
   },
   {
    "rev": 5,
    "node": "13207e5a10d9fd28ec424934298e176197f2c67f",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500000, 0],
    "desc": "new head",
    "bookmarks": [],
    "tags": [],
    "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"],
    "manifest": "4dc3def4f9b4c6e8de820f6ee74737f91e96a216",
    "extra": {"branch": "default"},
    "modified": [],
    "added": ["d"],
    "removed": []
   },
   {
    "rev": 4,
    "node": "07fa1db1064879a32157227401eb44b322ae53ce",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1400000, 0],
    "desc": "new branch",
    "bookmarks": ["foo"],
    "tags": [],
    "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"],
    "manifest": "cb5a1327723bada42f117e4c55a303246eaf9ccc",
    "extra": {"branch": "default"},
    "modified": [],
    "added": [],
    "removed": []
   },
   {
    "rev": 3,
    "node": "10e46f2dcbf4823578cf180f33ecf0b957964c47",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1300000, 0],
    "desc": "no user, no domain",
    "bookmarks": [],
    "tags": [],
    "parents": ["97054abb4ab824450e9164180baf491ae0078465"],
    "manifest": "cb5a1327723bada42f117e4c55a303246eaf9ccc",
    "extra": {"branch": "default"},
    "modified": ["c"],
    "added": [],
    "removed": []
   },
   {
    "rev": 2,
    "node": "97054abb4ab824450e9164180baf491ae0078465",
    "branch": "default",
    "phase": "draft",
    "user": "other@place",
    "date": [1200000, 0],
    "desc": "no person",
    "bookmarks": [],
    "tags": [],
    "parents": ["b608e9d1a3f0273ccf70fb85fd6866b3482bf965"],
    "manifest": "6e0e82995c35d0d57a52aca8da4e56139e06b4b1",
    "extra": {"branch": "default"},
    "modified": [],
    "added": ["c"],
    "removed": []
   },
   {
    "rev": 1,
    "node": "b608e9d1a3f0273ccf70fb85fd6866b3482bf965",
    "branch": "default",
    "phase": "draft",
    "user": "A. N. Other <other@place>",
    "date": [1100000, 0],
    "desc": "other 1\nother 2\n\nother 3",
    "bookmarks": [],
    "tags": [],
    "parents": ["1e4e1b8f71e05681d422154f5421e385fec3454f"],
    "manifest": "4e8d705b1e53e3f9375e0e60dc7b525d8211fe55",
    "extra": {"branch": "default"},
    "modified": [],
    "added": ["b"],
    "removed": []
   },
   {
    "rev": 0,
    "node": "1e4e1b8f71e05681d422154f5421e385fec3454f",
    "branch": "default",
    "phase": "draft",
    "user": "User Name <user@hostname>",
    "date": [1000000, 0],
    "desc": "line 1\nline 2",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"],
    "manifest": "a0c8bcbbb45c63b90b70ad007bf38961f64f2af0",
    "extra": {"branch": "default"},
    "modified": [],
    "added": ["a"],
    "removed": []
   }
  ]

Error if style not readable:

#if unix-permissions no-root
  $ touch q
  $ chmod 0 q
  $ hg log --style ./q
  abort: Permission denied: ./q
  [255]
#endif

Error if no style:

  $ hg log --style notexist
  abort: style 'notexist' not found
  (available styles: bisect, changelog, compact, default, phases, show, status, xml)
  [255]

  $ hg log -T list
  available styles: bisect, changelog, compact, default, phases, show, status, xml
  abort: specify a template
  [255]

Error if style missing key:

  $ echo 'q = q' > t
  $ hg log --style ./t
  abort: "changeset" not in template map
  [255]

Error if style missing value:

  $ echo 'changeset =' > t
  $ hg log --style t
  hg: parse error at t:1: missing value
  [255]

Error if include fails:

  $ echo 'changeset = q' >> t
#if unix-permissions no-root
  $ hg log --style ./t
  abort: template file ./q: Permission denied
  [255]
  $ rm -f q
#endif

Include works:

  $ echo '{rev}' > q
  $ hg log --style ./t
  8
  7
  6
  5
  4
  3
  2
  1
  0

Check that recursive reference does not fall into RuntimeError (issue4758):

 common mistake:

  $ cat << EOF > issue4758
  > changeset = '{changeset}\n'
  > EOF
  $ hg log --style ./issue4758
  abort: recursive reference 'changeset' in template
  [255]

 circular reference:

  $ cat << EOF > issue4758
  > changeset = '{foo}'
  > foo = '{changeset}'
  > EOF
  $ hg log --style ./issue4758
  abort: recursive reference 'foo' in template
  [255]

 buildmap() -> gettemplate(), where no thunk was made:

  $ cat << EOF > issue4758
  > changeset = '{files % changeset}\n'
  > EOF
  $ hg log --style ./issue4758
  abort: recursive reference 'changeset' in template
  [255]

 not a recursion if a keyword of the same name exists:

  $ cat << EOF > issue4758
  > changeset = '{tags % rev}'
  > rev = '{rev} {tag}\n'
  > EOF
  $ hg log --style ./issue4758 -r tip
  8 tip

Check that {phase} works correctly on parents:

  $ cat << EOF > parentphase
  > changeset_debug = '{rev} ({phase}):{parents}\n'
  > parent = ' {rev} ({phase})'
  > EOF
  $ hg phase -r 5 --public
  $ hg phase -r 7 --secret --force
  $ hg log --debug -G --style ./parentphase
  @  8 (secret): 7 (secret) -1 (public)
  |
  o  7 (secret): -1 (public) -1 (public)
  
  o    6 (draft): 5 (public) 4 (draft)
  |\
  | o  5 (public): 3 (public) -1 (public)
  | |
  o |  4 (draft): 3 (public) -1 (public)
  |/
  o  3 (public): 2 (public) -1 (public)
  |
  o  2 (public): 1 (public) -1 (public)
  |
  o  1 (public): 0 (public) -1 (public)
  |
  o  0 (public): -1 (public) -1 (public)
  

Missing non-standard names give no error (backward compatibility):

  $ echo "changeset = '{c}'" > t
  $ hg log --style ./t

Defining non-standard name works:

  $ cat <<EOF > t
  > changeset = '{c}'
  > c = q
  > EOF
  $ hg log --style ./t
  8
  7
  6
  5
  4
  3
  2
  1
  0

ui.style works:

  $ echo '[ui]' > .hg/hgrc
  $ echo 'style = t' >> .hg/hgrc
  $ hg log
  8
  7
  6
  5
  4
  3
  2
  1
  0


Issue338:

  $ hg log --style=changelog > changelog

  $ cat changelog
  2020-01-01  test  <test>
  
  	* fourth, second, third:
  	third
  	[95c24699272e] [tip]
  
  1970-01-12  User Name  <user@hostname>
  
  	* second:
  	second
  	[29114dbae42b]
  
  1970-01-18  person  <person>
  
  	* merge
  	[f7e5795620e7]
  
  	* d:
  	new head
  	[13207e5a10d9]
  
  1970-01-17  person  <person>
  
  	* new branch
  	[07fa1db10648]
  
  1970-01-16  person  <person>
  
  	* c:
  	no user, no domain
  	[10e46f2dcbf4]
  
  1970-01-14  other  <other@place>
  
  	* c:
  	no person
  	[97054abb4ab8]
  
  1970-01-13  A. N. Other  <other@place>
  
  	* b:
  	other 1 other 2
  
  	other 3
  	[b608e9d1a3f0]
  
  1970-01-12  User Name  <user@hostname>
  
  	* a:
  	line 1 line 2
  	[1e4e1b8f71e0]
  

Issue2130: xml output for 'hg heads' is malformed

  $ hg heads --style changelog
  2020-01-01  test  <test>
  
  	* fourth, second, third:
  	third
  	[95c24699272e] [tip]
  
  1970-01-18  person  <person>
  
  	* merge
  	[f7e5795620e7]
  

Keys work:

  $ for key in author branch branches date desc file_adds file_dels file_mods \
  >         file_copies file_copies_switch files \
  >         manifest node parents rev tags diffstat extras \
  >         p1rev p2rev p1node p2node; do
  >     for mode in '' --verbose --debug; do
  >         hg log $mode --template "$key$mode: {$key}\n"
  >     done
  > done
  author: test
  author: User Name <user@hostname>
  author: person
  author: person
  author: person
  author: person
  author: other@place
  author: A. N. Other <other@place>
  author: User Name <user@hostname>
  author--verbose: test
  author--verbose: User Name <user@hostname>
  author--verbose: person
  author--verbose: person
  author--verbose: person
  author--verbose: person
  author--verbose: other@place
  author--verbose: A. N. Other <other@place>
  author--verbose: User Name <user@hostname>
  author--debug: test
  author--debug: User Name <user@hostname>
  author--debug: person
  author--debug: person
  author--debug: person
  author--debug: person
  author--debug: other@place
  author--debug: A. N. Other <other@place>
  author--debug: User Name <user@hostname>
  branch: default
  branch: default
  branch: default
  branch: default
  branch: default
  branch: default
  branch: default
  branch: default
  branch: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branches: 
  branches: 
  branches: 
  branches: 
  branches: 
  branches: 
  branches: 
  branches: 
  branches: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  date: 1577872860.00
  date: 1000000.00
  date: 1500001.00
  date: 1500000.00
  date: 1400000.00
  date: 1300000.00
  date: 1200000.00
  date: 1100000.00
  date: 1000000.00
  date--verbose: 1577872860.00
  date--verbose: 1000000.00
  date--verbose: 1500001.00
  date--verbose: 1500000.00
  date--verbose: 1400000.00
  date--verbose: 1300000.00
  date--verbose: 1200000.00
  date--verbose: 1100000.00
  date--verbose: 1000000.00
  date--debug: 1577872860.00
  date--debug: 1000000.00
  date--debug: 1500001.00
  date--debug: 1500000.00
  date--debug: 1400000.00
  date--debug: 1300000.00
  date--debug: 1200000.00
  date--debug: 1100000.00
  date--debug: 1000000.00
  desc: third
  desc: second
  desc: merge
  desc: new head
  desc: new branch
  desc: no user, no domain
  desc: no person
  desc: other 1
  other 2
  
  other 3
  desc: line 1
  line 2
  desc--verbose: third
  desc--verbose: second
  desc--verbose: merge
  desc--verbose: new head
  desc--verbose: new branch
  desc--verbose: no user, no domain
  desc--verbose: no person
  desc--verbose: other 1
  other 2
  
  other 3
  desc--verbose: line 1
  line 2
  desc--debug: third
  desc--debug: second
  desc--debug: merge
  desc--debug: new head
  desc--debug: new branch
  desc--debug: no user, no domain
  desc--debug: no person
  desc--debug: other 1
  other 2
  
  other 3
  desc--debug: line 1
  line 2
  file_adds: fourth third
  file_adds: second
  file_adds: 
  file_adds: d
  file_adds: 
  file_adds: 
  file_adds: c
  file_adds: b
  file_adds: a
  file_adds--verbose: fourth third
  file_adds--verbose: second
  file_adds--verbose: 
  file_adds--verbose: d
  file_adds--verbose: 
  file_adds--verbose: 
  file_adds--verbose: c
  file_adds--verbose: b
  file_adds--verbose: a
  file_adds--debug: fourth third
  file_adds--debug: second
  file_adds--debug: 
  file_adds--debug: d
  file_adds--debug: 
  file_adds--debug: 
  file_adds--debug: c
  file_adds--debug: b
  file_adds--debug: a
  file_dels: second
  file_dels: 
  file_dels: 
  file_dels: 
  file_dels: 
  file_dels: 
  file_dels: 
  file_dels: 
  file_dels: 
  file_dels--verbose: second
  file_dels--verbose: 
  file_dels--verbose: 
  file_dels--verbose: 
  file_dels--verbose: 
  file_dels--verbose: 
  file_dels--verbose: 
  file_dels--verbose: 
  file_dels--verbose: 
  file_dels--debug: second
  file_dels--debug: 
  file_dels--debug: 
  file_dels--debug: 
  file_dels--debug: 
  file_dels--debug: 
  file_dels--debug: 
  file_dels--debug: 
  file_dels--debug: 
  file_mods: 
  file_mods: 
  file_mods: 
  file_mods: 
  file_mods: 
  file_mods: c
  file_mods: 
  file_mods: 
  file_mods: 
  file_mods--verbose: 
  file_mods--verbose: 
  file_mods--verbose: 
  file_mods--verbose: 
  file_mods--verbose: 
  file_mods--verbose: c
  file_mods--verbose: 
  file_mods--verbose: 
  file_mods--verbose: 
  file_mods--debug: 
  file_mods--debug: 
  file_mods--debug: 
  file_mods--debug: 
  file_mods--debug: 
  file_mods--debug: c
  file_mods--debug: 
  file_mods--debug: 
  file_mods--debug: 
  file_copies: fourth (second)
  file_copies: 
  file_copies: 
  file_copies: 
  file_copies: 
  file_copies: 
  file_copies: 
  file_copies: 
  file_copies: 
  file_copies--verbose: fourth (second)
  file_copies--verbose: 
  file_copies--verbose: 
  file_copies--verbose: 
  file_copies--verbose: 
  file_copies--verbose: 
  file_copies--verbose: 
  file_copies--verbose: 
  file_copies--verbose: 
  file_copies--debug: fourth (second)
  file_copies--debug: 
  file_copies--debug: 
  file_copies--debug: 
  file_copies--debug: 
  file_copies--debug: 
  file_copies--debug: 
  file_copies--debug: 
  file_copies--debug: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--verbose: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  file_copies_switch--debug: 
  files: fourth second third
  files: second
  files: 
  files: d
  files: 
  files: c
  files: c
  files: b
  files: a
  files--verbose: fourth second third
  files--verbose: second
  files--verbose: 
  files--verbose: d
  files--verbose: 
  files--verbose: c
  files--verbose: c
  files--verbose: b
  files--verbose: a
  files--debug: fourth second third
  files--debug: second
  files--debug: 
  files--debug: d
  files--debug: 
  files--debug: c
  files--debug: c
  files--debug: b
  files--debug: a
  manifest: 6:94961b75a2da
  manifest: 5:f2dbc354b94e
  manifest: 4:4dc3def4f9b4
  manifest: 4:4dc3def4f9b4
  manifest: 3:cb5a1327723b
  manifest: 3:cb5a1327723b
  manifest: 2:6e0e82995c35
  manifest: 1:4e8d705b1e53
  manifest: 0:a0c8bcbbb45c
  manifest--verbose: 6:94961b75a2da
  manifest--verbose: 5:f2dbc354b94e
  manifest--verbose: 4:4dc3def4f9b4
  manifest--verbose: 4:4dc3def4f9b4
  manifest--verbose: 3:cb5a1327723b
  manifest--verbose: 3:cb5a1327723b
  manifest--verbose: 2:6e0e82995c35
  manifest--verbose: 1:4e8d705b1e53
  manifest--verbose: 0:a0c8bcbbb45c
  manifest--debug: 6:94961b75a2da554b4df6fb599e5bfc7d48de0c64
  manifest--debug: 5:f2dbc354b94e5ec0b4f10680ee0cee816101d0bf
  manifest--debug: 4:4dc3def4f9b4c6e8de820f6ee74737f91e96a216
  manifest--debug: 4:4dc3def4f9b4c6e8de820f6ee74737f91e96a216
  manifest--debug: 3:cb5a1327723bada42f117e4c55a303246eaf9ccc
  manifest--debug: 3:cb5a1327723bada42f117e4c55a303246eaf9ccc
  manifest--debug: 2:6e0e82995c35d0d57a52aca8da4e56139e06b4b1
  manifest--debug: 1:4e8d705b1e53e3f9375e0e60dc7b525d8211fe55
  manifest--debug: 0:a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  node: 95c24699272ef57d062b8bccc32c878bf841784a
  node: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  node: f7e5795620e78993ad76680c4306bb2da83907b3
  node: 13207e5a10d9fd28ec424934298e176197f2c67f
  node: 07fa1db1064879a32157227401eb44b322ae53ce
  node: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  node: 97054abb4ab824450e9164180baf491ae0078465
  node: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  node: 1e4e1b8f71e05681d422154f5421e385fec3454f
  node--verbose: 95c24699272ef57d062b8bccc32c878bf841784a
  node--verbose: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  node--verbose: f7e5795620e78993ad76680c4306bb2da83907b3
  node--verbose: 13207e5a10d9fd28ec424934298e176197f2c67f
  node--verbose: 07fa1db1064879a32157227401eb44b322ae53ce
  node--verbose: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  node--verbose: 97054abb4ab824450e9164180baf491ae0078465
  node--verbose: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  node--verbose: 1e4e1b8f71e05681d422154f5421e385fec3454f
  node--debug: 95c24699272ef57d062b8bccc32c878bf841784a
  node--debug: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  node--debug: f7e5795620e78993ad76680c4306bb2da83907b3
  node--debug: 13207e5a10d9fd28ec424934298e176197f2c67f
  node--debug: 07fa1db1064879a32157227401eb44b322ae53ce
  node--debug: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  node--debug: 97054abb4ab824450e9164180baf491ae0078465
  node--debug: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  node--debug: 1e4e1b8f71e05681d422154f5421e385fec3454f
  parents: 
  parents: -1:000000000000 
  parents: 5:13207e5a10d9 4:07fa1db10648 
  parents: 3:10e46f2dcbf4 
  parents: 
  parents: 
  parents: 
  parents: 
  parents: 
  parents--verbose: 
  parents--verbose: -1:000000000000 
  parents--verbose: 5:13207e5a10d9 4:07fa1db10648 
  parents--verbose: 3:10e46f2dcbf4 
  parents--verbose: 
  parents--verbose: 
  parents--verbose: 
  parents--verbose: 
  parents--verbose: 
  parents--debug: 7:29114dbae42b9f078cf2714dbe3a86bba8ec7453 -1:0000000000000000000000000000000000000000 
  parents--debug: -1:0000000000000000000000000000000000000000 -1:0000000000000000000000000000000000000000 
  parents--debug: 5:13207e5a10d9fd28ec424934298e176197f2c67f 4:07fa1db1064879a32157227401eb44b322ae53ce 
  parents--debug: 3:10e46f2dcbf4823578cf180f33ecf0b957964c47 -1:0000000000000000000000000000000000000000 
  parents--debug: 3:10e46f2dcbf4823578cf180f33ecf0b957964c47 -1:0000000000000000000000000000000000000000 
  parents--debug: 2:97054abb4ab824450e9164180baf491ae0078465 -1:0000000000000000000000000000000000000000 
  parents--debug: 1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965 -1:0000000000000000000000000000000000000000 
  parents--debug: 0:1e4e1b8f71e05681d422154f5421e385fec3454f -1:0000000000000000000000000000000000000000 
  parents--debug: -1:0000000000000000000000000000000000000000 -1:0000000000000000000000000000000000000000 
  rev: 8
  rev: 7
  rev: 6
  rev: 5
  rev: 4
  rev: 3
  rev: 2
  rev: 1
  rev: 0
  rev--verbose: 8
  rev--verbose: 7
  rev--verbose: 6
  rev--verbose: 5
  rev--verbose: 4
  rev--verbose: 3
  rev--verbose: 2
  rev--verbose: 1
  rev--verbose: 0
  rev--debug: 8
  rev--debug: 7
  rev--debug: 6
  rev--debug: 5
  rev--debug: 4
  rev--debug: 3
  rev--debug: 2
  rev--debug: 1
  rev--debug: 0
  tags: tip
  tags: 
  tags: 
  tags: 
  tags: 
  tags: 
  tags: 
  tags: 
  tags: 
  tags--verbose: tip
  tags--verbose: 
  tags--verbose: 
  tags--verbose: 
  tags--verbose: 
  tags--verbose: 
  tags--verbose: 
  tags--verbose: 
  tags--verbose: 
  tags--debug: tip
  tags--debug: 
  tags--debug: 
  tags--debug: 
  tags--debug: 
  tags--debug: 
  tags--debug: 
  tags--debug: 
  tags--debug: 
  diffstat: 3: +2/-1
  diffstat: 1: +1/-0
  diffstat: 0: +0/-0
  diffstat: 1: +1/-0
  diffstat: 0: +0/-0
  diffstat: 1: +1/-0
  diffstat: 1: +4/-0
  diffstat: 1: +2/-0
  diffstat: 1: +1/-0
  diffstat--verbose: 3: +2/-1
  diffstat--verbose: 1: +1/-0
  diffstat--verbose: 0: +0/-0
  diffstat--verbose: 1: +1/-0
  diffstat--verbose: 0: +0/-0
  diffstat--verbose: 1: +1/-0
  diffstat--verbose: 1: +4/-0
  diffstat--verbose: 1: +2/-0
  diffstat--verbose: 1: +1/-0
  diffstat--debug: 3: +2/-1
  diffstat--debug: 1: +1/-0
  diffstat--debug: 0: +0/-0
  diffstat--debug: 1: +1/-0
  diffstat--debug: 0: +0/-0
  diffstat--debug: 1: +1/-0
  diffstat--debug: 1: +4/-0
  diffstat--debug: 1: +2/-0
  diffstat--debug: 1: +1/-0
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  p1rev: 7
  p1rev: -1
  p1rev: 5
  p1rev: 3
  p1rev: 3
  p1rev: 2
  p1rev: 1
  p1rev: 0
  p1rev: -1
  p1rev--verbose: 7
  p1rev--verbose: -1
  p1rev--verbose: 5
  p1rev--verbose: 3
  p1rev--verbose: 3
  p1rev--verbose: 2
  p1rev--verbose: 1
  p1rev--verbose: 0
  p1rev--verbose: -1
  p1rev--debug: 7
  p1rev--debug: -1
  p1rev--debug: 5
  p1rev--debug: 3
  p1rev--debug: 3
  p1rev--debug: 2
  p1rev--debug: 1
  p1rev--debug: 0
  p1rev--debug: -1
  p2rev: -1
  p2rev: -1
  p2rev: 4
  p2rev: -1
  p2rev: -1
  p2rev: -1
  p2rev: -1
  p2rev: -1
  p2rev: -1
  p2rev--verbose: -1
  p2rev--verbose: -1
  p2rev--verbose: 4
  p2rev--verbose: -1
  p2rev--verbose: -1
  p2rev--verbose: -1
  p2rev--verbose: -1
  p2rev--verbose: -1
  p2rev--verbose: -1
  p2rev--debug: -1
  p2rev--debug: -1
  p2rev--debug: 4
  p2rev--debug: -1
  p2rev--debug: -1
  p2rev--debug: -1
  p2rev--debug: -1
  p2rev--debug: -1
  p2rev--debug: -1
  p1node: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  p1node: 0000000000000000000000000000000000000000
  p1node: 13207e5a10d9fd28ec424934298e176197f2c67f
  p1node: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  p1node: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  p1node: 97054abb4ab824450e9164180baf491ae0078465
  p1node: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  p1node: 1e4e1b8f71e05681d422154f5421e385fec3454f
  p1node: 0000000000000000000000000000000000000000
  p1node--verbose: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  p1node--verbose: 0000000000000000000000000000000000000000
  p1node--verbose: 13207e5a10d9fd28ec424934298e176197f2c67f
  p1node--verbose: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  p1node--verbose: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  p1node--verbose: 97054abb4ab824450e9164180baf491ae0078465
  p1node--verbose: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  p1node--verbose: 1e4e1b8f71e05681d422154f5421e385fec3454f
  p1node--verbose: 0000000000000000000000000000000000000000
  p1node--debug: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  p1node--debug: 0000000000000000000000000000000000000000
  p1node--debug: 13207e5a10d9fd28ec424934298e176197f2c67f
  p1node--debug: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  p1node--debug: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  p1node--debug: 97054abb4ab824450e9164180baf491ae0078465
  p1node--debug: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  p1node--debug: 1e4e1b8f71e05681d422154f5421e385fec3454f
  p1node--debug: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 07fa1db1064879a32157227401eb44b322ae53ce
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 07fa1db1064879a32157227401eb44b322ae53ce
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 07fa1db1064879a32157227401eb44b322ae53ce
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000

Filters work:

  $ hg log --template '{author|domain}\n'
  
  hostname
  
  
  
  
  place
  place
  hostname

  $ hg log --template '{author|person}\n'
  test
  User Name
  person
  person
  person
  person
  other
  A. N. Other
  User Name

  $ hg log --template '{author|user}\n'
  test
  user
  person
  person
  person
  person
  other
  other
  user

  $ hg log --template '{date|date}\n'
  Wed Jan 01 10:01:00 2020 +0000
  Mon Jan 12 13:46:40 1970 +0000
  Sun Jan 18 08:40:01 1970 +0000
  Sun Jan 18 08:40:00 1970 +0000
  Sat Jan 17 04:53:20 1970 +0000
  Fri Jan 16 01:06:40 1970 +0000
  Wed Jan 14 21:20:00 1970 +0000
  Tue Jan 13 17:33:20 1970 +0000
  Mon Jan 12 13:46:40 1970 +0000

  $ hg log --template '{date|isodate}\n'
  2020-01-01 10:01 +0000
  1970-01-12 13:46 +0000
  1970-01-18 08:40 +0000
  1970-01-18 08:40 +0000
  1970-01-17 04:53 +0000
  1970-01-16 01:06 +0000
  1970-01-14 21:20 +0000
  1970-01-13 17:33 +0000
  1970-01-12 13:46 +0000

  $ hg log --template '{date|isodatesec}\n'
  2020-01-01 10:01:00 +0000
  1970-01-12 13:46:40 +0000
  1970-01-18 08:40:01 +0000
  1970-01-18 08:40:00 +0000
  1970-01-17 04:53:20 +0000
  1970-01-16 01:06:40 +0000
  1970-01-14 21:20:00 +0000
  1970-01-13 17:33:20 +0000
  1970-01-12 13:46:40 +0000

  $ hg log --template '{date|rfc822date}\n'
  Wed, 01 Jan 2020 10:01:00 +0000
  Mon, 12 Jan 1970 13:46:40 +0000
  Sun, 18 Jan 1970 08:40:01 +0000
  Sun, 18 Jan 1970 08:40:00 +0000
  Sat, 17 Jan 1970 04:53:20 +0000
  Fri, 16 Jan 1970 01:06:40 +0000
  Wed, 14 Jan 1970 21:20:00 +0000
  Tue, 13 Jan 1970 17:33:20 +0000
  Mon, 12 Jan 1970 13:46:40 +0000

  $ hg log --template '{desc|firstline}\n'
  third
  second
  merge
  new head
  new branch
  no user, no domain
  no person
  other 1
  line 1

  $ hg log --template '{node|short}\n'
  95c24699272e
  29114dbae42b
  f7e5795620e7
  13207e5a10d9
  07fa1db10648
  10e46f2dcbf4
  97054abb4ab8
  b608e9d1a3f0
  1e4e1b8f71e0

  $ hg log --template '<changeset author="{author|xmlescape}"/>\n'
  <changeset author="test"/>
  <changeset author="User Name &lt;user@hostname&gt;"/>
  <changeset author="person"/>
  <changeset author="person"/>
  <changeset author="person"/>
  <changeset author="person"/>
  <changeset author="other@place"/>
  <changeset author="A. N. Other &lt;other@place&gt;"/>
  <changeset author="User Name &lt;user@hostname&gt;"/>

  $ hg log --template '{rev}: {children}\n'
  8: 
  7: 8:95c24699272e
  6: 
  5: 6:f7e5795620e7
  4: 6:f7e5795620e7
  3: 4:07fa1db10648 5:13207e5a10d9
  2: 3:10e46f2dcbf4
  1: 2:97054abb4ab8
  0: 1:b608e9d1a3f0

Formatnode filter works:

  $ hg -q log -r 0 --template '{node|formatnode}\n'
  1e4e1b8f71e0

  $ hg log -r 0 --template '{node|formatnode}\n'
  1e4e1b8f71e0

  $ hg -v log -r 0 --template '{node|formatnode}\n'
  1e4e1b8f71e0

  $ hg --debug log -r 0 --template '{node|formatnode}\n'
  1e4e1b8f71e05681d422154f5421e385fec3454f

Age filter:

  $ hg init unstable-hash
  $ cd unstable-hash
  $ hg log --template '{date|age}\n' > /dev/null || exit 1

  >>> from __future__ import absolute_import
  >>> import datetime
  >>> fp = open('a', 'w')
  >>> n = datetime.datetime.now() + datetime.timedelta(366 * 7)
  >>> fp.write('%d-%d-%d 00:00' % (n.year, n.month, n.day))
  >>> fp.close()
  $ hg add a
  $ hg commit -m future -d "`cat a`"

  $ hg log -l1 --template '{date|age}\n'
  7 years from now

  $ cd ..
  $ rm -rf unstable-hash

Add a dummy commit to make up for the instability of the above:

  $ echo a > a
  $ hg add a
  $ hg ci -m future

Count filter:

  $ hg log -l1 --template '{node|count} {node|short|count}\n'
  40 12

  $ hg log -l1 --template '{revset("null^")|count} {revset(".")|count} {revset("0::3")|count}\n'
  0 1 4

  $ hg log -G --template '{rev}: children: {children|count}, \
  > tags: {tags|count}, file_adds: {file_adds|count}, \
  > ancestors: {revset("ancestors(%s)", rev)|count}'
  @  9: children: 0, tags: 1, file_adds: 1, ancestors: 3
  |
  o  8: children: 1, tags: 0, file_adds: 2, ancestors: 2
  |
  o  7: children: 1, tags: 0, file_adds: 1, ancestors: 1
  
  o    6: children: 0, tags: 0, file_adds: 0, ancestors: 7
  |\
  | o  5: children: 1, tags: 0, file_adds: 1, ancestors: 5
  | |
  o |  4: children: 1, tags: 0, file_adds: 0, ancestors: 5
  |/
  o  3: children: 2, tags: 0, file_adds: 0, ancestors: 4
  |
  o  2: children: 1, tags: 0, file_adds: 1, ancestors: 3
  |
  o  1: children: 1, tags: 0, file_adds: 1, ancestors: 2
  |
  o  0: children: 1, tags: 0, file_adds: 1, ancestors: 1
  

Upper/lower filters:

  $ hg log -r0 --template '{author|upper}\n'
  USER NAME <USER@HOSTNAME>
  $ hg log -r0 --template '{author|lower}\n'
  user name <user@hostname>
  $ hg log -r0 --template '{date|upper}\n'
  abort: template filter 'upper' is not compatible with keyword 'date'
  [255]

Add a commit that does all possible modifications at once

  $ echo modify >> third
  $ touch b
  $ hg add b
  $ hg mv fourth fifth
  $ hg rm a
  $ hg ci -m "Modify, add, remove, rename"

Check the status template

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > color=
  > EOF

  $ hg log -T status -r 10
  changeset:   10:0f9759ec227a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Modify, add, remove, rename
  files:
  M third
  A b
  A fifth
  R a
  R fourth
  
  $ hg log -T status -C -r 10
  changeset:   10:0f9759ec227a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Modify, add, remove, rename
  files:
  M third
  A b
  A fifth
    fourth
  R a
  R fourth
  
  $ hg log -T status -C -r 10 -v
  changeset:   10:0f9759ec227a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  description:
  Modify, add, remove, rename
  
  files:
  M third
  A b
  A fifth
    fourth
  R a
  R fourth
  
  $ hg log -T status -C -r 10 --debug
  changeset:   10:0f9759ec227a4859c2014a345cd8a859022b7c6c
  tag:         tip
  phase:       secret
  parent:      9:bf9dfba36635106d6a73ccc01e28b762da60e066
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    89dd546f2de0a9d6d664f58d86097eb97baba567
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  description:
  Modify, add, remove, rename
  
  files:
  M third
  A b
  A fifth
    fourth
  R a
  R fourth
  
  $ hg log -T status -C -r 10 --quiet
  10:0f9759ec227a
  $ hg --color=debug log -T status -r 10
  [log.changeset changeset.secret|changeset:   10:0f9759ec227a]
  [log.tag|tag:         tip]
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [log.summary|summary:     Modify, add, remove, rename]
  [ui.note log.files|files:]
  [status.modified|M third]
  [status.added|A b]
  [status.added|A fifth]
  [status.removed|R a]
  [status.removed|R fourth]
  
  $ hg --color=debug log -T status -C -r 10
  [log.changeset changeset.secret|changeset:   10:0f9759ec227a]
  [log.tag|tag:         tip]
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [log.summary|summary:     Modify, add, remove, rename]
  [ui.note log.files|files:]
  [status.modified|M third]
  [status.added|A b]
  [status.added|A fifth]
  [status.copied|  fourth]
  [status.removed|R a]
  [status.removed|R fourth]
  
  $ hg --color=debug log -T status -C -r 10 -v
  [log.changeset changeset.secret|changeset:   10:0f9759ec227a]
  [log.tag|tag:         tip]
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [ui.note log.description|description:]
  [ui.note log.description|Modify, add, remove, rename]
  
  [ui.note log.files|files:]
  [status.modified|M third]
  [status.added|A b]
  [status.added|A fifth]
  [status.copied|  fourth]
  [status.removed|R a]
  [status.removed|R fourth]
  
  $ hg --color=debug log -T status -C -r 10 --debug
  [log.changeset changeset.secret|changeset:   10:0f9759ec227a4859c2014a345cd8a859022b7c6c]
  [log.tag|tag:         tip]
  [log.phase|phase:       secret]
  [log.parent changeset.secret|parent:      9:bf9dfba36635106d6a73ccc01e28b762da60e066]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    89dd546f2de0a9d6d664f58d86097eb97baba567]
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [ui.debug log.extra|extra:       branch=default]
  [ui.note log.description|description:]
  [ui.note log.description|Modify, add, remove, rename]
  
  [ui.note log.files|files:]
  [status.modified|M third]
  [status.added|A b]
  [status.added|A fifth]
  [status.copied|  fourth]
  [status.removed|R a]
  [status.removed|R fourth]
  
  $ hg --color=debug log -T status -C -r 10 --quiet
  [log.node|10:0f9759ec227a]

Check the bisect template

  $ hg bisect -g 1
  $ hg bisect -b 3 --noupdate
  Testing changeset 2:97054abb4ab8 (2 changesets remaining, ~1 tests)
  $ hg log -T bisect -r 0:4
  changeset:   0:1e4e1b8f71e0
  bisect:      good (implicit)
  user:        User Name <user@hostname>
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     line 1
  
  changeset:   1:b608e9d1a3f0
  bisect:      good
  user:        A. N. Other <other@place>
  date:        Tue Jan 13 17:33:20 1970 +0000
  summary:     other 1
  
  changeset:   2:97054abb4ab8
  bisect:      untested
  user:        other@place
  date:        Wed Jan 14 21:20:00 1970 +0000
  summary:     no person
  
  changeset:   3:10e46f2dcbf4
  bisect:      bad
  user:        person
  date:        Fri Jan 16 01:06:40 1970 +0000
  summary:     no user, no domain
  
  changeset:   4:07fa1db10648
  bisect:      bad (implicit)
  bookmark:    foo
  user:        person
  date:        Sat Jan 17 04:53:20 1970 +0000
  summary:     new branch
  
  $ hg log --debug -T bisect -r 0:4
  changeset:   0:1e4e1b8f71e05681d422154f5421e385fec3454f
  bisect:      good (implicit)
  phase:       public
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  user:        User Name <user@hostname>
  date:        Mon Jan 12 13:46:40 1970 +0000
  files+:      a
  extra:       branch=default
  description:
  line 1
  line 2
  
  
  changeset:   1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  bisect:      good
  phase:       public
  parent:      0:1e4e1b8f71e05681d422154f5421e385fec3454f
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4e8d705b1e53e3f9375e0e60dc7b525d8211fe55
  user:        A. N. Other <other@place>
  date:        Tue Jan 13 17:33:20 1970 +0000
  files+:      b
  extra:       branch=default
  description:
  other 1
  other 2
  
  other 3
  
  
  changeset:   2:97054abb4ab824450e9164180baf491ae0078465
  bisect:      untested
  phase:       public
  parent:      1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    6e0e82995c35d0d57a52aca8da4e56139e06b4b1
  user:        other@place
  date:        Wed Jan 14 21:20:00 1970 +0000
  files+:      c
  extra:       branch=default
  description:
  no person
  
  
  changeset:   3:10e46f2dcbf4823578cf180f33ecf0b957964c47
  bisect:      bad
  phase:       public
  parent:      2:97054abb4ab824450e9164180baf491ae0078465
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc
  user:        person
  date:        Fri Jan 16 01:06:40 1970 +0000
  files:       c
  extra:       branch=default
  description:
  no user, no domain
  
  
  changeset:   4:07fa1db1064879a32157227401eb44b322ae53ce
  bisect:      bad (implicit)
  bookmark:    foo
  phase:       draft
  parent:      3:10e46f2dcbf4823578cf180f33ecf0b957964c47
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc
  user:        person
  date:        Sat Jan 17 04:53:20 1970 +0000
  extra:       branch=default
  description:
  new branch
  
  
  $ hg log -v -T bisect -r 0:4
  changeset:   0:1e4e1b8f71e0
  bisect:      good (implicit)
  user:        User Name <user@hostname>
  date:        Mon Jan 12 13:46:40 1970 +0000
  files:       a
  description:
  line 1
  line 2
  
  
  changeset:   1:b608e9d1a3f0
  bisect:      good
  user:        A. N. Other <other@place>
  date:        Tue Jan 13 17:33:20 1970 +0000
  files:       b
  description:
  other 1
  other 2
  
  other 3
  
  
  changeset:   2:97054abb4ab8
  bisect:      untested
  user:        other@place
  date:        Wed Jan 14 21:20:00 1970 +0000
  files:       c
  description:
  no person
  
  
  changeset:   3:10e46f2dcbf4
  bisect:      bad
  user:        person
  date:        Fri Jan 16 01:06:40 1970 +0000
  files:       c
  description:
  no user, no domain
  
  
  changeset:   4:07fa1db10648
  bisect:      bad (implicit)
  bookmark:    foo
  user:        person
  date:        Sat Jan 17 04:53:20 1970 +0000
  description:
  new branch
  
  
  $ hg --color=debug log -T bisect -r 0:4
  [log.changeset changeset.public|changeset:   0:1e4e1b8f71e0]
  [log.bisect bisect.good|bisect:      good (implicit)]
  [log.user|user:        User Name <user@hostname>]
  [log.date|date:        Mon Jan 12 13:46:40 1970 +0000]
  [log.summary|summary:     line 1]
  
  [log.changeset changeset.public|changeset:   1:b608e9d1a3f0]
  [log.bisect bisect.good|bisect:      good]
  [log.user|user:        A. N. Other <other@place>]
  [log.date|date:        Tue Jan 13 17:33:20 1970 +0000]
  [log.summary|summary:     other 1]
  
  [log.changeset changeset.public|changeset:   2:97054abb4ab8]
  [log.bisect bisect.untested|bisect:      untested]
  [log.user|user:        other@place]
  [log.date|date:        Wed Jan 14 21:20:00 1970 +0000]
  [log.summary|summary:     no person]
  
  [log.changeset changeset.public|changeset:   3:10e46f2dcbf4]
  [log.bisect bisect.bad|bisect:      bad]
  [log.user|user:        person]
  [log.date|date:        Fri Jan 16 01:06:40 1970 +0000]
  [log.summary|summary:     no user, no domain]
  
  [log.changeset changeset.draft|changeset:   4:07fa1db10648]
  [log.bisect bisect.bad|bisect:      bad (implicit)]
  [log.bookmark|bookmark:    foo]
  [log.user|user:        person]
  [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
  [log.summary|summary:     new branch]
  
  $ hg --color=debug log --debug -T bisect -r 0:4
  [log.changeset changeset.public|changeset:   0:1e4e1b8f71e05681d422154f5421e385fec3454f]
  [log.bisect bisect.good|bisect:      good (implicit)]
  [log.phase|phase:       public]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    a0c8bcbbb45c63b90b70ad007bf38961f64f2af0]
  [log.user|user:        User Name <user@hostname>]
  [log.date|date:        Mon Jan 12 13:46:40 1970 +0000]
  [ui.debug log.files|files+:      a]
  [ui.debug log.extra|extra:       branch=default]
  [ui.note log.description|description:]
  [ui.note log.description|line 1
  line 2]
  
  
  [log.changeset changeset.public|changeset:   1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965]
  [log.bisect bisect.good|bisect:      good]
  [log.phase|phase:       public]
  [log.parent changeset.public|parent:      0:1e4e1b8f71e05681d422154f5421e385fec3454f]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    4e8d705b1e53e3f9375e0e60dc7b525d8211fe55]
  [log.user|user:        A. N. Other <other@place>]
  [log.date|date:        Tue Jan 13 17:33:20 1970 +0000]
  [ui.debug log.files|files+:      b]
  [ui.debug log.extra|extra:       branch=default]
  [ui.note log.description|description:]
  [ui.note log.description|other 1
  other 2
  
  other 3]
  
  
  [log.changeset changeset.public|changeset:   2:97054abb4ab824450e9164180baf491ae0078465]
  [log.bisect bisect.untested|bisect:      untested]
  [log.phase|phase:       public]
  [log.parent changeset.public|parent:      1:b608e9d1a3f0273ccf70fb85fd6866b3482bf965]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    6e0e82995c35d0d57a52aca8da4e56139e06b4b1]
  [log.user|user:        other@place]
  [log.date|date:        Wed Jan 14 21:20:00 1970 +0000]
  [ui.debug log.files|files+:      c]
  [ui.debug log.extra|extra:       branch=default]
  [ui.note log.description|description:]
  [ui.note log.description|no person]
  
  
  [log.changeset changeset.public|changeset:   3:10e46f2dcbf4823578cf180f33ecf0b957964c47]
  [log.bisect bisect.bad|bisect:      bad]
  [log.phase|phase:       public]
  [log.parent changeset.public|parent:      2:97054abb4ab824450e9164180baf491ae0078465]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc]
  [log.user|user:        person]
  [log.date|date:        Fri Jan 16 01:06:40 1970 +0000]
  [ui.debug log.files|files:       c]
  [ui.debug log.extra|extra:       branch=default]
  [ui.note log.description|description:]
  [ui.note log.description|no user, no domain]
  
  
  [log.changeset changeset.draft|changeset:   4:07fa1db1064879a32157227401eb44b322ae53ce]
  [log.bisect bisect.bad|bisect:      bad (implicit)]
  [log.bookmark|bookmark:    foo]
  [log.phase|phase:       draft]
  [log.parent changeset.public|parent:      3:10e46f2dcbf4823578cf180f33ecf0b957964c47]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    cb5a1327723bada42f117e4c55a303246eaf9ccc]
  [log.user|user:        person]
  [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
  [ui.debug log.extra|extra:       branch=default]
  [ui.note log.description|description:]
  [ui.note log.description|new branch]
  
  
  $ hg --color=debug log -v -T bisect -r 0:4
  [log.changeset changeset.public|changeset:   0:1e4e1b8f71e0]
  [log.bisect bisect.good|bisect:      good (implicit)]
  [log.user|user:        User Name <user@hostname>]
  [log.date|date:        Mon Jan 12 13:46:40 1970 +0000]
  [ui.note log.files|files:       a]
  [ui.note log.description|description:]
  [ui.note log.description|line 1
  line 2]
  
  
  [log.changeset changeset.public|changeset:   1:b608e9d1a3f0]
  [log.bisect bisect.good|bisect:      good]
  [log.user|user:        A. N. Other <other@place>]
  [log.date|date:        Tue Jan 13 17:33:20 1970 +0000]
  [ui.note log.files|files:       b]
  [ui.note log.description|description:]
  [ui.note log.description|other 1
  other 2
  
  other 3]
  
  
  [log.changeset changeset.public|changeset:   2:97054abb4ab8]
  [log.bisect bisect.untested|bisect:      untested]
  [log.user|user:        other@place]
  [log.date|date:        Wed Jan 14 21:20:00 1970 +0000]
  [ui.note log.files|files:       c]
  [ui.note log.description|description:]
  [ui.note log.description|no person]
  
  
  [log.changeset changeset.public|changeset:   3:10e46f2dcbf4]
  [log.bisect bisect.bad|bisect:      bad]
  [log.user|user:        person]
  [log.date|date:        Fri Jan 16 01:06:40 1970 +0000]
  [ui.note log.files|files:       c]
  [ui.note log.description|description:]
  [ui.note log.description|no user, no domain]
  
  
  [log.changeset changeset.draft|changeset:   4:07fa1db10648]
  [log.bisect bisect.bad|bisect:      bad (implicit)]
  [log.bookmark|bookmark:    foo]
  [log.user|user:        person]
  [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
  [ui.note log.description|description:]
  [ui.note log.description|new branch]
  
  
  $ hg bisect --reset

Error on syntax:

  $ echo 'x = "f' >> t
  $ hg log
  hg: parse error at t:3: unmatched quotes
  [255]

  $ hg log -T '{date'
  hg: parse error at 1: unterminated template expansion
  ({date
   ^ here)
  [255]

Behind the scenes, this will throw TypeError

  $ hg log -l 3 --template '{date|obfuscate}\n'
  abort: template filter 'obfuscate' is not compatible with keyword 'date'
  [255]

Behind the scenes, this will throw a ValueError

  $ hg log -l 3 --template 'line: {desc|shortdate}\n'
  abort: template filter 'shortdate' is not compatible with keyword 'desc'
  [255]

Behind the scenes, this will throw AttributeError

  $ hg log -l 3 --template 'line: {date|escape}\n'
  abort: template filter 'escape' is not compatible with keyword 'date'
  [255]

  $ hg log -l 3 --template 'line: {extras|localdate}\n'
  hg: parse error: localdate expects a date information
  [255]

Behind the scenes, this will throw ValueError

  $ hg tip --template '{author|email|date}\n'
  hg: parse error: date expects a date information
  [255]

  $ hg tip -T '{author|email|shortdate}\n'
  abort: template filter 'shortdate' is not compatible with keyword 'author'
  [255]

  $ hg tip -T '{get(extras, "branch")|shortdate}\n'
  abort: incompatible use of template filter 'shortdate'
  [255]

Error in nested template:

  $ hg log -T '{"date'
  hg: parse error at 2: unterminated string
  ({"date
    ^ here)
  [255]

  $ hg log -T '{"foo{date|?}"}'
  hg: parse error at 11: syntax error
  ({"foo{date|?}"}
             ^ here)
  [255]

Thrown an error if a template function doesn't exist

  $ hg tip --template '{foo()}\n'
  hg: parse error: unknown function 'foo'
  [255]

Pass generator object created by template function to filter

  $ hg log -l 1 --template '{if(author, author)|user}\n'
  test

Test index keyword:

  $ hg log -l 2 -T '{index + 10}{files % " {index}:{file}"}\n'
  10 0:a 1:b 2:fifth 3:fourth 4:third
  11 0:a

Test diff function:

  $ hg diff -c 8
  diff -r 29114dbae42b -r 95c24699272e fourth
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
  @@ -0,0 +1,1 @@
  +second
  diff -r 29114dbae42b -r 95c24699272e second
  --- a/second	Mon Jan 12 13:46:40 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -second
  diff -r 29114dbae42b -r 95c24699272e third
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/third	Wed Jan 01 10:01:00 2020 +0000
  @@ -0,0 +1,1 @@
  +third

  $ hg log -r 8 -T "{diff()}"
  diff -r 29114dbae42b -r 95c24699272e fourth
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
  @@ -0,0 +1,1 @@
  +second
  diff -r 29114dbae42b -r 95c24699272e second
  --- a/second	Mon Jan 12 13:46:40 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -second
  diff -r 29114dbae42b -r 95c24699272e third
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/third	Wed Jan 01 10:01:00 2020 +0000
  @@ -0,0 +1,1 @@
  +third

  $ hg log -r 8 -T "{diff('glob:f*')}"
  diff -r 29114dbae42b -r 95c24699272e fourth
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
  @@ -0,0 +1,1 @@
  +second

  $ hg log -r 8 -T "{diff('', 'glob:f*')}"
  diff -r 29114dbae42b -r 95c24699272e second
  --- a/second	Mon Jan 12 13:46:40 1970 +0000
  +++ /dev/null	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +0,0 @@
  -second
  diff -r 29114dbae42b -r 95c24699272e third
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/third	Wed Jan 01 10:01:00 2020 +0000
  @@ -0,0 +1,1 @@
  +third

  $ hg log -r 8 -T "{diff('FOURTH'|lower)}"
  diff -r 29114dbae42b -r 95c24699272e fourth
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/fourth	Wed Jan 01 10:01:00 2020 +0000
  @@ -0,0 +1,1 @@
  +second

ui verbosity:

  $ hg log -l1 -T '{verbosity}\n'
  
  $ hg log -l1 -T '{verbosity}\n' --debug
  debug
  $ hg log -l1 -T '{verbosity}\n' --quiet
  quiet
  $ hg log -l1 -T '{verbosity}\n' --verbose
  verbose

  $ cd ..


latesttag:

  $ hg init latesttag
  $ cd latesttag

  $ echo a > file
  $ hg ci -Am a -d '0 0'
  adding file

  $ echo b >> file
  $ hg ci -m b -d '1 0'

  $ echo c >> head1
  $ hg ci -Am h1c -d '2 0'
  adding head1

  $ hg update -q 1
  $ echo d >> head2
  $ hg ci -Am h2d -d '3 0'
  adding head2

  $ echo e >> head2
  $ hg ci -m h2e -d '4 0'

  $ hg merge -q
  $ hg ci -m merge -d '5 -3600'

No tag set:

  $ hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\n'
  @    5: null+5
  |\
  | o  4: null+4
  | |
  | o  3: null+3
  | |
  o |  2: null+3
  |/
  o  1: null+2
  |
  o  0: null+1
  

One common tag: longest path wins for {latesttagdistance}:

  $ hg tag -r 1 -m t1 -d '6 0' t1
  $ hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\n'
  @  6: t1+4
  |
  o    5: t1+3
  |\
  | o  4: t1+2
  | |
  | o  3: t1+1
  | |
  o |  2: t1+1
  |/
  o  1: t1+0
  |
  o  0: null+1
  

One ancestor tag: closest wins:

  $ hg tag -r 2 -m t2 -d '7 0' t2
  $ hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\n'
  @  7: t2+3
  |
  o  6: t2+2
  |
  o    5: t2+1
  |\
  | o  4: t1+2
  | |
  | o  3: t1+1
  | |
  o |  2: t2+0
  |/
  o  1: t1+0
  |
  o  0: null+1
  

Two branch tags: more recent wins if same number of changes:

  $ hg tag -r 3 -m t3 -d '8 0' t3
  $ hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\n'
  @  8: t3+5
  |
  o  7: t3+4
  |
  o  6: t3+3
  |
  o    5: t3+2
  |\
  | o  4: t3+1
  | |
  | o  3: t3+0
  | |
  o |  2: t2+0
  |/
  o  1: t1+0
  |
  o  0: null+1
  

Two branch tags: fewest changes wins:

  $ hg tag -r 4 -m t4 -d '4 0' t4 # older than t2, but should not matter
  $ hg log -G --template "{rev}: {latesttag % '{tag}+{distance},{changes} '}\n"
  @  9: t4+5,6
  |
  o  8: t4+4,5
  |
  o  7: t4+3,4
  |
  o  6: t4+2,3
  |
  o    5: t4+1,2
  |\
  | o  4: t4+0,0
  | |
  | o  3: t3+0,0
  | |
  o |  2: t2+0,0
  |/
  o  1: t1+0,0
  |
  o  0: null+1,1
  

Merged tag overrides:

  $ hg tag -r 5 -m t5 -d '9 0' t5
  $ hg tag -r 3 -m at3 -d '10 0' at3
  $ hg log -G --template '{rev}: {latesttag}+{latesttagdistance}\n'
  @  11: t5+6
  |
  o  10: t5+5
  |
  o  9: t5+4
  |
  o  8: t5+3
  |
  o  7: t5+2
  |
  o  6: t5+1
  |
  o    5: t5+0
  |\
  | o  4: t4+0
  | |
  | o  3: at3:t3+0
  | |
  o |  2: t2+0
  |/
  o  1: t1+0
  |
  o  0: null+1
  

  $ hg log -G --template "{rev}: {latesttag % '{tag}+{distance},{changes} '}\n"
  @  11: t5+6,6
  |
  o  10: t5+5,5
  |
  o  9: t5+4,4
  |
  o  8: t5+3,3
  |
  o  7: t5+2,2
  |
  o  6: t5+1,1
  |
  o    5: t5+0,0
  |\
  | o  4: t4+0,0
  | |
  | o  3: at3+0,0 t3+0,0
  | |
  o |  2: t2+0,0
  |/
  o  1: t1+0,0
  |
  o  0: null+1,1
  

  $ hg log -G --template "{rev}: {latesttag('re:^t[13]$') % '{tag}, C: {changes}, D: {distance}'}\n"
  @  11: t3, C: 9, D: 8
  |
  o  10: t3, C: 8, D: 7
  |
  o  9: t3, C: 7, D: 6
  |
  o  8: t3, C: 6, D: 5
  |
  o  7: t3, C: 5, D: 4
  |
  o  6: t3, C: 4, D: 3
  |
  o    5: t3, C: 3, D: 2
  |\
  | o  4: t3, C: 1, D: 1
  | |
  | o  3: t3, C: 0, D: 0
  | |
  o |  2: t1, C: 1, D: 1
  |/
  o  1: t1, C: 0, D: 0
  |
  o  0: null, C: 1, D: 1
  

  $ cd ..


Style path expansion: issue1948 - ui.style option doesn't work on OSX
if it is a relative path

  $ mkdir -p home/styles

  $ cat > home/styles/teststyle <<EOF
  > changeset = 'test {rev}:{node|short}\n'
  > EOF

  $ HOME=`pwd`/home; export HOME

  $ cat > latesttag/.hg/hgrc <<EOF
  > [ui]
  > style = ~/styles/teststyle
  > EOF

  $ hg -R latesttag tip
  test 11:97e5943b523a

Test recursive showlist template (issue1989):

  $ cat > style1989 <<EOF
  > changeset = '{file_mods}{manifest}{extras}'
  > file_mod  = 'M|{author|person}\n'
  > manifest = '{rev},{author}\n'
  > extra = '{key}: {author}\n'
  > EOF

  $ hg -R latesttag log -r tip --style=style1989
  M|test
  11,test
  branch: test

Test new-style inline templating:

  $ hg log -R latesttag -r tip --template 'modified files: {file_mods % " {file}\n"}\n'
  modified files:  .hgtags
  

  $ hg log -R latesttag -r tip -T '{rev % "a"}\n'
  hg: parse error: keyword 'rev' is not iterable
  [255]
  $ hg log -R latesttag -r tip -T '{get(extras, "unknown") % "a"}\n'
  hg: parse error: None is not iterable
  [255]

Test new-style inline templating of non-list/dict type:

  $ hg log -R latesttag -r tip -T '{manifest}\n'
  11:2bc6e9006ce2
  $ hg log -R latesttag -r tip -T 'string length: {manifest|count}\n'
  string length: 15
  $ hg log -R latesttag -r tip -T '{manifest % "{rev}:{node}"}\n'
  11:2bc6e9006ce29882383a22d39fd1f4e66dd3e2fc

  $ hg log -R latesttag -r tip -T '{get(extras, "branch") % "{key}: {value}\n"}'
  branch: default
  $ hg log -R latesttag -r tip -T '{get(extras, "unknown") % "{key}\n"}'
  hg: parse error: None is not iterable
  [255]
  $ hg log -R latesttag -r tip -T '{min(extras) % "{key}: {value}\n"}'
  branch: default
  $ hg log -R latesttag -l1 -T '{min(revset("0:9")) % "{rev}:{node|short}\n"}'
  0:ce3cec86e6c2
  $ hg log -R latesttag -l1 -T '{max(revset("0:9")) % "{rev}:{node|short}\n"}'
  9:fbc7cd862e9c

Test manifest/get() can be join()-ed as before, though it's silly:

  $ hg log -R latesttag -r tip -T '{join(manifest, "")}\n'
  11:2bc6e9006ce2
  $ hg log -R latesttag -r tip -T '{join(get(extras, "branch"), "")}\n'
  default

Test min/max of integers

  $ hg log -R latesttag -l1 -T '{min(revset("9:10"))}\n'
  9
  $ hg log -R latesttag -l1 -T '{max(revset("9:10"))}\n'
  10

Test dot operator precedence:

  $ hg debugtemplate -R latesttag -r0 -v '{manifest.node|short}\n'
  (template
    (|
      (.
        (symbol 'manifest')
        (symbol 'node'))
      (symbol 'short'))
    (string '\n'))
  89f4071fec70

 (the following examples are invalid, but seem natural in parsing POV)

  $ hg debugtemplate -R latesttag -r0 -v '{foo|bar.baz}\n' 2> /dev/null
  (template
    (|
      (symbol 'foo')
      (.
        (symbol 'bar')
        (symbol 'baz')))
    (string '\n'))
  [255]
  $ hg debugtemplate -R latesttag -r0 -v '{foo.bar()}\n' 2> /dev/null
  (template
    (.
      (symbol 'foo')
      (func
        (symbol 'bar')
        None))
    (string '\n'))
  [255]

Test evaluation of dot operator:

  $ hg log -R latesttag -l1 -T '{min(revset("0:9")).node}\n'
  ce3cec86e6c26bd9bdfc590a6b92abc9680f1796
  $ hg log -R latesttag -r0 -T '{extras.branch}\n'
  default

  $ hg log -R latesttag -l1 -T '{author.invalid}\n'
  hg: parse error: keyword 'author' has no member
  [255]
  $ hg log -R latesttag -l1 -T '{min("abc").invalid}\n'
  hg: parse error: 'a' has no member
  [255]

Test the sub function of templating for expansion:

  $ hg log -R latesttag -r 10 --template '{sub("[0-9]", "x", "{rev}")}\n'
  xx

  $ hg log -R latesttag -r 10 -T '{sub("[", "x", rev)}\n'
  hg: parse error: sub got an invalid pattern: [
  [255]
  $ hg log -R latesttag -r 10 -T '{sub("[0-9]", r"\1", rev)}\n'
  hg: parse error: sub got an invalid replacement: \1
  [255]

Test the strip function with chars specified:

  $ hg log -R latesttag --template '{desc}\n'
  at3
  t5
  t4
  t3
  t2
  t1
  merge
  h2e
  h2d
  h1c
  b
  a

  $ hg log -R latesttag --template '{strip(desc, "te")}\n'
  at3
  5
  4
  3
  2
  1
  merg
  h2
  h2d
  h1c
  b
  a

Test date format:

  $ hg log -R latesttag --template 'date: {date(date, "%y %m %d %S %z")}\n'
  date: 70 01 01 10 +0000
  date: 70 01 01 09 +0000
  date: 70 01 01 04 +0000
  date: 70 01 01 08 +0000
  date: 70 01 01 07 +0000
  date: 70 01 01 06 +0000
  date: 70 01 01 05 +0100
  date: 70 01 01 04 +0000
  date: 70 01 01 03 +0000
  date: 70 01 01 02 +0000
  date: 70 01 01 01 +0000
  date: 70 01 01 00 +0000

Test invalid date:

  $ hg log -R latesttag -T '{date(rev)}\n'
  hg: parse error: date expects a date information
  [255]

Test integer literal:

  $ hg debugtemplate -v '{(0)}\n'
  (template
    (group
      (integer '0'))
    (string '\n'))
  0
  $ hg debugtemplate -v '{(123)}\n'
  (template
    (group
      (integer '123'))
    (string '\n'))
  123
  $ hg debugtemplate -v '{(-4)}\n'
  (template
    (group
      (negate
        (integer '4')))
    (string '\n'))
  -4
  $ hg debugtemplate '{(-)}\n'
  hg: parse error at 3: not a prefix: )
  ({(-)}\n
     ^ here)
  [255]
  $ hg debugtemplate '{(-a)}\n'
  hg: parse error: negation needs an integer argument
  [255]

top-level integer literal is interpreted as symbol (i.e. variable name):

  $ hg debugtemplate -D 1=one -v '{1}\n'
  (template
    (integer '1')
    (string '\n'))
  one
  $ hg debugtemplate -D 1=one -v '{if("t", "{1}")}\n'
  (template
    (func
      (symbol 'if')
      (list
        (string 't')
        (template
          (integer '1'))))
    (string '\n'))
  one
  $ hg debugtemplate -D 1=one -v '{1|stringify}\n'
  (template
    (|
      (integer '1')
      (symbol 'stringify'))
    (string '\n'))
  one

unless explicit symbol is expected:

  $ hg log -Ra -r0 -T '{desc|1}\n'
  hg: parse error: expected a symbol, got 'integer'
  [255]
  $ hg log -Ra -r0 -T '{1()}\n'
  hg: parse error: expected a symbol, got 'integer'
  [255]

Test string literal:

  $ hg debugtemplate -Ra -r0 -v '{"string with no template fragment"}\n'
  (template
    (string 'string with no template fragment')
    (string '\n'))
  string with no template fragment
  $ hg debugtemplate -Ra -r0 -v '{"template: {rev}"}\n'
  (template
    (template
      (string 'template: ')
      (symbol 'rev'))
    (string '\n'))
  template: 0
  $ hg debugtemplate -Ra -r0 -v '{r"rawstring: {rev}"}\n'
  (template
    (string 'rawstring: {rev}')
    (string '\n'))
  rawstring: {rev}
  $ hg debugtemplate -Ra -r0 -v '{files % r"rawstring: {file}"}\n'
  (template
    (%
      (symbol 'files')
      (string 'rawstring: {file}'))
    (string '\n'))
  rawstring: {file}

Test string escaping:

  $ hg log -R latesttag -r 0 --template '>\n<>\\n<{if(rev, "[>\n<>\\n<]")}>\n<>\\n<\n'
  >
  <>\n<[>
  <>\n<]>
  <>\n<

  $ hg log -R latesttag -r 0 \
  > --config ui.logtemplate='>\n<>\\n<{if(rev, "[>\n<>\\n<]")}>\n<>\\n<\n'
  >
  <>\n<[>
  <>\n<]>
  <>\n<

  $ hg log -R latesttag -r 0 -T esc \
  > --config templates.esc='>\n<>\\n<{if(rev, "[>\n<>\\n<]")}>\n<>\\n<\n'
  >
  <>\n<[>
  <>\n<]>
  <>\n<

  $ cat <<'EOF' > esctmpl
  > changeset = '>\n<>\\n<{if(rev, "[>\n<>\\n<]")}>\n<>\\n<\n'
  > EOF
  $ hg log -R latesttag -r 0 --style ./esctmpl
  >
  <>\n<[>
  <>\n<]>
  <>\n<

Test string escaping of quotes:

  $ hg log -Ra -r0 -T '{"\""}\n'
  "
  $ hg log -Ra -r0 -T '{"\\\""}\n'
  \"
  $ hg log -Ra -r0 -T '{r"\""}\n'
  \"
  $ hg log -Ra -r0 -T '{r"\\\""}\n'
  \\\"


  $ hg log -Ra -r0 -T '{"\""}\n'
  "
  $ hg log -Ra -r0 -T '{"\\\""}\n'
  \"
  $ hg log -Ra -r0 -T '{r"\""}\n'
  \"
  $ hg log -Ra -r0 -T '{r"\\\""}\n'
  \\\"

Test exception in quoted template. single backslash before quotation mark is
stripped before parsing:

  $ cat <<'EOF' > escquotetmpl
  > changeset = "\" \\" \\\" \\\\" {files % \"{file}\"}\n"
  > EOF
  $ cd latesttag
  $ hg log -r 2 --style ../escquotetmpl
  " \" \" \\" head1

  $ hg log -r 2 -T esc --config templates.esc='"{\"valid\"}\n"'
  valid
  $ hg log -r 2 -T esc --config templates.esc="'"'{\'"'"'valid\'"'"'}\n'"'"
  valid

Test compatibility with 2.9.2-3.4 of escaped quoted strings in nested
_evalifliteral() templates (issue4733):

  $ hg log -r 2 -T '{if(rev, "\"{rev}")}\n'
  "2
  $ hg log -r 2 -T '{if(rev, "{if(rev, \"\\\"{rev}\")}")}\n'
  "2
  $ hg log -r 2 -T '{if(rev, "{if(rev, \"{if(rev, \\\"\\\\\\\"{rev}\\\")}\")}")}\n'
  "2

  $ hg log -r 2 -T '{if(rev, "\\\"")}\n'
  \"
  $ hg log -r 2 -T '{if(rev, "{if(rev, \"\\\\\\\"\")}")}\n'
  \"
  $ hg log -r 2 -T '{if(rev, "{if(rev, \"{if(rev, \\\"\\\\\\\\\\\\\\\"\\\")}\")}")}\n'
  \"

  $ hg log -r 2 -T '{if(rev, r"\\\"")}\n'
  \\\"
  $ hg log -r 2 -T '{if(rev, "{if(rev, r\"\\\\\\\"\")}")}\n'
  \\\"
  $ hg log -r 2 -T '{if(rev, "{if(rev, \"{if(rev, r\\\"\\\\\\\\\\\\\\\"\\\")}\")}")}\n'
  \\\"

escaped single quotes and errors:

  $ hg log -r 2 -T "{if(rev, '{if(rev, \'foo\')}')}"'\n'
  foo
  $ hg log -r 2 -T "{if(rev, '{if(rev, r\'foo\')}')}"'\n'
  foo
  $ hg log -r 2 -T '{if(rev, "{if(rev, \")}")}\n'
  hg: parse error at 21: unterminated string
  ({if(rev, "{if(rev, \")}")}\n
                       ^ here)
  [255]
  $ hg log -r 2 -T '{if(rev, \"\\"")}\n'
  hg: parse error: trailing \ in string
  [255]
  $ hg log -r 2 -T '{if(rev, r\"\\"")}\n'
  hg: parse error: trailing \ in string
  [255]

  $ cd ..

Test leading backslashes:

  $ cd latesttag
  $ hg log -r 2 -T '\{rev} {files % "\{file}"}\n'
  {rev} {file}
  $ hg log -r 2 -T '\\{rev} {files % "\\{file}"}\n'
  \2 \head1
  $ hg log -r 2 -T '\\\{rev} {files % "\\\{file}"}\n'
  \{rev} \{file}
  $ cd ..

Test leading backslashes in "if" expression (issue4714):

  $ cd latesttag
  $ hg log -r 2 -T '{if("1", "\{rev}")} {if("1", r"\{rev}")}\n'
  {rev} \{rev}
  $ hg log -r 2 -T '{if("1", "\\{rev}")} {if("1", r"\\{rev}")}\n'
  \2 \\{rev}
  $ hg log -r 2 -T '{if("1", "\\\{rev}")} {if("1", r"\\\{rev}")}\n'
  \{rev} \\\{rev}
  $ cd ..

"string-escape"-ed "\x5c\x786e" becomes r"\x6e" (once) or r"n" (twice)

  $ hg log -R a -r 0 --template '{if("1", "\x5c\x786e", "NG")}\n'
  \x6e
  $ hg log -R a -r 0 --template '{if("1", r"\x5c\x786e", "NG")}\n'
  \x5c\x786e
  $ hg log -R a -r 0 --template '{if("", "NG", "\x5c\x786e")}\n'
  \x6e
  $ hg log -R a -r 0 --template '{if("", "NG", r"\x5c\x786e")}\n'
  \x5c\x786e

  $ hg log -R a -r 2 --template '{ifeq("no perso\x6e", desc, "\x5c\x786e", "NG")}\n'
  \x6e
  $ hg log -R a -r 2 --template '{ifeq(r"no perso\x6e", desc, "NG", r"\x5c\x786e")}\n'
  \x5c\x786e
  $ hg log -R a -r 2 --template '{ifeq(desc, "no perso\x6e", "\x5c\x786e", "NG")}\n'
  \x6e
  $ hg log -R a -r 2 --template '{ifeq(desc, r"no perso\x6e", "NG", r"\x5c\x786e")}\n'
  \x5c\x786e

  $ hg log -R a -r 8 --template '{join(files, "\n")}\n'
  fourth
  second
  third
  $ hg log -R a -r 8 --template '{join(files, r"\n")}\n'
  fourth\nsecond\nthird

  $ hg log -R a -r 2 --template '{rstdoc("1st\n\n2nd", "htm\x6c")}'
  <p>
  1st
  </p>
  <p>
  2nd
  </p>
  $ hg log -R a -r 2 --template '{rstdoc(r"1st\n\n2nd", "html")}'
  <p>
  1st\n\n2nd
  </p>
  $ hg log -R a -r 2 --template '{rstdoc("1st\n\n2nd", r"htm\x6c")}'
  1st
  
  2nd

  $ hg log -R a -r 2 --template '{strip(desc, "\x6e")}\n'
  o perso
  $ hg log -R a -r 2 --template '{strip(desc, r"\x6e")}\n'
  no person
  $ hg log -R a -r 2 --template '{strip("no perso\x6e", "\x6e")}\n'
  o perso
  $ hg log -R a -r 2 --template '{strip(r"no perso\x6e", r"\x6e")}\n'
  no perso

  $ hg log -R a -r 2 --template '{sub("\\x6e", "\x2d", desc)}\n'
  -o perso-
  $ hg log -R a -r 2 --template '{sub(r"\\x6e", "-", desc)}\n'
  no person
  $ hg log -R a -r 2 --template '{sub("n", r"\x2d", desc)}\n'
  \x2do perso\x2d
  $ hg log -R a -r 2 --template '{sub("n", "\x2d", "no perso\x6e")}\n'
  -o perso-
  $ hg log -R a -r 2 --template '{sub("n", r"\x2d", r"no perso\x6e")}\n'
  \x2do perso\x6e

  $ hg log -R a -r 8 --template '{files % "{file}\n"}'
  fourth
  second
  third

Test string escaping in nested expression:

  $ hg log -R a -r 8 --template '{ifeq(r"\x6e", if("1", "\x5c\x786e"), join(files, "\x5c\x786e"))}\n'
  fourth\x6esecond\x6ethird
  $ hg log -R a -r 8 --template '{ifeq(if("1", r"\x6e"), "\x5c\x786e", join(files, "\x5c\x786e"))}\n'
  fourth\x6esecond\x6ethird

  $ hg log -R a -r 8 --template '{join(files, ifeq(branch, "default", "\x5c\x786e"))}\n'
  fourth\x6esecond\x6ethird
  $ hg log -R a -r 8 --template '{join(files, ifeq(branch, "default", r"\x5c\x786e"))}\n'
  fourth\x5c\x786esecond\x5c\x786ethird

  $ hg log -R a -r 3:4 --template '{rev}:{sub(if("1", "\x6e"), ifeq(branch, "foo", r"\x5c\x786e", "\x5c\x786e"), desc)}\n'
  3:\x6eo user, \x6eo domai\x6e
  4:\x6eew bra\x6ech

Test quotes in nested expression are evaluated just like a $(command)
substitution in POSIX shells:

  $ hg log -R a -r 8 -T '{"{"{rev}:{node|short}"}"}\n'
  8:95c24699272e
  $ hg log -R a -r 8 -T '{"{"\{{rev}} \"{node|short}\""}"}\n'
  {8} "95c24699272e"

Test recursive evaluation:

  $ hg init r
  $ cd r
  $ echo a > a
  $ hg ci -Am '{rev}'
  adding a
  $ hg log -r 0 --template '{if(rev, desc)}\n'
  {rev}
  $ hg log -r 0 --template '{if(rev, "{author} {rev}")}\n'
  test 0

  $ hg bookmark -q 'text.{rev}'
  $ echo aa >> aa
  $ hg ci -u '{node|short}' -m 'desc to be wrapped desc to be wrapped'

  $ hg log -l1 --template '{fill(desc, "20", author, bookmarks)}'
  {node|short}desc to
  text.{rev}be wrapped
  text.{rev}desc to be
  text.{rev}wrapped (no-eol)
  $ hg log -l1 --template '{fill(desc, "20", "{node|short}:", "text.{rev}:")}'
  ea4c0948489d:desc to
  text.1:be wrapped
  text.1:desc to be
  text.1:wrapped (no-eol)
  $ hg log -l1 -T '{fill(desc, date, "", "")}\n'
  hg: parse error: fill expects an integer width
  [255]

  $ COLUMNS=25 hg log -l1 --template '{fill(desc, termwidth, "{node|short}:", "termwidth.{rev}:")}'
  ea4c0948489d:desc to be
  termwidth.1:wrapped desc
  termwidth.1:to be wrapped (no-eol)

  $ hg log -l 1 --template '{sub(r"[0-9]", "-", author)}'
  {node|short} (no-eol)
  $ hg log -l 1 --template '{sub(r"[0-9]", "-", "{node|short}")}'
  ea-c-------d (no-eol)

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > color=
  > [color]
  > mode=ansi
  > text.{rev} = red
  > text.1 = green
  > EOF
  $ hg log --color=always -l 1 --template '{label(bookmarks, "text\n")}'
  \x1b[0;31mtext\x1b[0m (esc)
  $ hg log --color=always -l 1 --template '{label("text.{rev}", "text\n")}'
  \x1b[0;32mtext\x1b[0m (esc)

color effect can be specified without quoting:

  $ hg log --color=always -l 1 --template '{label(red, "text\n")}'
  \x1b[0;31mtext\x1b[0m (esc)

color effects can be nested (issue5413)

  $ hg debugtemplate --color=always \
  > '{label(red, "red{label(magenta, "ma{label(cyan, "cyan")}{label(yellow, "yellow")}genta")}")}\n'
  \x1b[0;31mred\x1b[0;35mma\x1b[0;36mcyan\x1b[0m\x1b[0;31m\x1b[0;35m\x1b[0;33myellow\x1b[0m\x1b[0;31m\x1b[0;35mgenta\x1b[0m (esc)

pad() should interact well with color codes (issue5416)

  $ hg debugtemplate --color=always \
  > '{pad(label(red, "red"), 5, label(cyan, "-"))}\n'
  \x1b[0;31mred\x1b[0m\x1b[0;36m-\x1b[0m\x1b[0;36m-\x1b[0m (esc)

label should be no-op if color is disabled:

  $ hg log --color=never -l 1 --template '{label(red, "text\n")}'
  text
  $ hg log --config extensions.color=! -l 1 --template '{label(red, "text\n")}'
  text

Test dict constructor:

  $ hg log -r 0 -T '{dict(y=node|short, x=rev)}\n'
  y=f7769ec2ab97 x=0
  $ hg log -r 0 -T '{dict(x=rev, y=node|short) % "{key}={value}\n"}'
  x=0
  y=f7769ec2ab97
  $ hg log -r 0 -T '{dict(x=rev, y=node|short)|json}\n'
  {"x": 0, "y": "f7769ec2ab97"}
  $ hg log -r 0 -T '{dict()|json}\n'
  {}

  $ hg log -r 0 -T '{dict(rev, node=node|short)}\n'
  rev=0 node=f7769ec2ab97
  $ hg log -r 0 -T '{dict(rev, node|short)}\n'
  rev=0 node=f7769ec2ab97

  $ hg log -r 0 -T '{dict(rev, rev=rev)}\n'
  hg: parse error: duplicated dict key 'rev' inferred
  [255]
  $ hg log -r 0 -T '{dict(node, node|short)}\n'
  hg: parse error: duplicated dict key 'node' inferred
  [255]
  $ hg log -r 0 -T '{dict(1 + 2)}'
  hg: parse error: dict key cannot be inferred
  [255]

  $ hg log -r 0 -T '{dict(x=rev, x=node)}'
  hg: parse error: dict got multiple values for keyword argument 'x'
  [255]

Test get function:

  $ hg log -r 0 --template '{get(extras, "branch")}\n'
  default
  $ hg log -r 0 --template '{get(extras, "br{"anch"}")}\n'
  default
  $ hg log -r 0 --template '{get(files, "should_fail")}\n'
  hg: parse error: get() expects a dict as first argument
  [255]

Test json filter applied to hybrid object:

  $ hg log -r0 -T '{files|json}\n'
  ["a"]
  $ hg log -r0 -T '{extras|json}\n'
  {"branch": "default"}

Test localdate(date, tz) function:

  $ TZ=JST-09 hg log -r0 -T '{date|localdate|isodate}\n'
  1970-01-01 09:00 +0900
  $ TZ=JST-09 hg log -r0 -T '{localdate(date, "UTC")|isodate}\n'
  1970-01-01 00:00 +0000
  $ TZ=JST-09 hg log -r0 -T '{localdate(date, "blahUTC")|isodate}\n'
  hg: parse error: localdate expects a timezone
  [255]
  $ TZ=JST-09 hg log -r0 -T '{localdate(date, "+0200")|isodate}\n'
  1970-01-01 02:00 +0200
  $ TZ=JST-09 hg log -r0 -T '{localdate(date, "0")|isodate}\n'
  1970-01-01 00:00 +0000
  $ TZ=JST-09 hg log -r0 -T '{localdate(date, 0)|isodate}\n'
  1970-01-01 00:00 +0000
  $ hg log -r0 -T '{localdate(date, "invalid")|isodate}\n'
  hg: parse error: localdate expects a timezone
  [255]
  $ hg log -r0 -T '{localdate(date, date)|isodate}\n'
  hg: parse error: localdate expects a timezone
  [255]

Test shortest(node) function:

  $ echo b > b
  $ hg ci -qAm b
  $ hg log --template '{shortest(node)}\n'
  21c1
  ea4c
  f776
  $ hg log --template '{shortest(node, 10)}\n'
  21c1b7ca5a
  ea4c094848
  f7769ec2ab
  $ hg log --template '{node|shortest}\n' -l1
  21c1

  $ hg log -r 0 -T '{shortest(node, "1{"0"}")}\n'
  f7769ec2ab
  $ hg log -r 0 -T '{shortest(node, "not an int")}\n'
  hg: parse error: shortest() expects an integer minlength
  [255]

  $ hg log -r 'wdir()' -T '{node|shortest}\n'
  ffff

  $ cd ..

Test shortest(node) with the repo having short hash collision:

  $ hg init hashcollision
  $ cd hashcollision
  $ cat <<EOF >> .hg/hgrc
  > [experimental]
  > evolution.createmarkers=True
  > EOF
  $ echo 0 > a
  $ hg ci -qAm 0
  $ for i in 17 129 248 242 480 580 617 1057 2857 4025; do
  >   hg up -q 0
  >   echo $i > a
  >   hg ci -qm $i
  > done
  $ hg up -q null
  $ hg log -r0: -T '{rev}:{node}\n'
  0:b4e73ffab476aa0ee32ed81ca51e07169844bc6a
  1:11424df6dc1dd4ea255eae2b58eaca7831973bbc
  2:11407b3f1b9c3e76a79c1ec5373924df096f0499
  3:11dd92fe0f39dfdaacdaa5f3997edc533875cfc4
  4:10776689e627b465361ad5c296a20a487e153ca4
  5:a00be79088084cb3aff086ab799f8790e01a976b
  6:a0b0acd79b4498d0052993d35a6a748dd51d13e6
  7:a0457b3450b8e1b778f1163b31a435802987fe5d
  8:c56256a09cd28e5764f32e8e2810d0f01e2e357a
  9:c5623987d205cd6d9d8389bfc40fff9dbb670b48
  10:c562ddd9c94164376c20b86b0b4991636a3bf84f
  $ hg debugobsolete a00be79088084cb3aff086ab799f8790e01a976b
  obsoleted 1 changesets
  $ hg debugobsolete c5623987d205cd6d9d8389bfc40fff9dbb670b48
  obsoleted 1 changesets
  $ hg debugobsolete c562ddd9c94164376c20b86b0b4991636a3bf84f
  obsoleted 1 changesets

 nodes starting with '11' (we don't have the revision number '11' though)

  $ hg log -r 1:3 -T '{rev}:{shortest(node, 0)}\n'
  1:1142
  2:1140
  3:11d

 '5:a00' is hidden, but still we have two nodes starting with 'a0'

  $ hg log -r 6:7 -T '{rev}:{shortest(node, 0)}\n'
  6:a0b
  7:a04

 node '10' conflicts with the revision number '10' even if it is hidden
 (we could exclude hidden revision numbers, but currently we don't)

  $ hg log -r 4 -T '{rev}:{shortest(node, 0)}\n'
  4:107
  $ hg log -r 4 -T '{rev}:{shortest(node, 0)}\n' --hidden
  4:107

 node 'c562' should be unique if the other 'c562' nodes are hidden
 (but we don't try the slow path to filter out hidden nodes for now)

  $ hg log -r 8 -T '{rev}:{node|shortest}\n'
  8:c5625
  $ hg log -r 8:10 -T '{rev}:{node|shortest}\n' --hidden
  8:c5625
  9:c5623
  10:c562d

  $ cd ..

Test pad function

  $ cd r

  $ hg log --template '{pad(rev, 20)} {author|user}\n'
  2                    test
  1                    {node|short}
  0                    test

  $ hg log --template '{pad(rev, 20, " ", True)} {author|user}\n'
                     2 test
                     1 {node|short}
                     0 test

  $ hg log --template '{pad(rev, 20, "-", False)} {author|user}\n'
  2------------------- test
  1------------------- {node|short}
  0------------------- test

Test unicode fillchar

  $ HGENCODING=utf-8 hg log -r 0 -T '{pad("hello", 10, "☃")}world\n'
  hello☃☃☃☃☃world

Test template string in pad function

  $ hg log -r 0 -T '{pad("\{{rev}}", 10)} {author|user}\n'
  {0}        test

  $ hg log -r 0 -T '{pad(r"\{rev}", 10)} {author|user}\n'
  \{rev}     test

Test width argument passed to pad function

  $ hg log -r 0 -T '{pad(rev, "1{"0"}")} {author|user}\n'
  0          test
  $ hg log -r 0 -T '{pad(rev, "not an int")}\n'
  hg: parse error: pad() expects an integer width
  [255]

Test invalid fillchar passed to pad function

  $ hg log -r 0 -T '{pad(rev, 10, "")}\n'
  hg: parse error: pad() expects a single fill character
  [255]
  $ hg log -r 0 -T '{pad(rev, 10, "--")}\n'
  hg: parse error: pad() expects a single fill character
  [255]

Test boolean argument passed to pad function

 no crash

  $ hg log -r 0 -T '{pad(rev, 10, "-", "f{"oo"}")}\n'
  ---------0

 string/literal

  $ hg log -r 0 -T '{pad(rev, 10, "-", "false")}\n'
  ---------0
  $ hg log -r 0 -T '{pad(rev, 10, "-", false)}\n'
  0---------
  $ hg log -r 0 -T '{pad(rev, 10, "-", "")}\n'
  0---------

 unknown keyword is evaluated to ''

  $ hg log -r 0 -T '{pad(rev, 10, "-", unknownkeyword)}\n'
  0---------

Test separate function

  $ hg log -r 0 -T '{separate("-", "", "a", "b", "", "", "c", "")}\n'
  a-b-c
  $ hg log -r 0 -T '{separate(" ", "{rev}:{node|short}", author|user, bookmarks)}\n'
  0:f7769ec2ab97 test
  $ hg log -r 0 --color=always -T '{separate(" ", "a", label(red, "b"), "c", label(red, ""), "d")}\n'
  a \x1b[0;31mb\x1b[0m c d (esc)

Test boolean expression/literal passed to if function

  $ hg log -r 0 -T '{if(rev, "rev 0 is True")}\n'
  rev 0 is True
  $ hg log -r 0 -T '{if(0, "literal 0 is True as well")}\n'
  literal 0 is True as well
  $ hg log -r 0 -T '{if("", "", "empty string is False")}\n'
  empty string is False
  $ hg log -r 0 -T '{if(revset(r"0 - 0"), "", "empty list is False")}\n'
  empty list is False
  $ hg log -r 0 -T '{if(true, "true is True")}\n'
  true is True
  $ hg log -r 0 -T '{if(false, "", "false is False")}\n'
  false is False
  $ hg log -r 0 -T '{if("false", "non-empty string is True")}\n'
  non-empty string is True

Test ifcontains function

  $ hg log --template '{rev} {ifcontains(rev, "2 two 0", "is in the string", "is not")}\n'
  2 is in the string
  1 is not
  0 is in the string

  $ hg log -T '{rev} {ifcontains(rev, "2 two{" 0"}", "is in the string", "is not")}\n'
  2 is in the string
  1 is not
  0 is in the string

  $ hg log --template '{rev} {ifcontains("a", file_adds, "added a", "did not add a")}\n'
  2 did not add a
  1 did not add a
  0 added a

  $ hg log --debug -T '{rev}{ifcontains(1, parents, " is parent of 1")}\n'
  2 is parent of 1
  1
  0

Test revset function

  $ hg log --template '{rev} {ifcontains(rev, revset("."), "current rev", "not current rev")}\n'
  2 current rev
  1 not current rev
  0 not current rev

  $ hg log --template '{rev} {ifcontains(rev, revset(". + .^"), "match rev", "not match rev")}\n'
  2 match rev
  1 match rev
  0 not match rev

  $ hg log -T '{ifcontains(desc, revset(":"), "", "type not match")}\n' -l1
  type not match

  $ hg log --template '{rev} Parents: {revset("parents(%s)", rev)}\n'
  2 Parents: 1
  1 Parents: 0
  0 Parents: 

  $ cat >> .hg/hgrc <<EOF
  > [revsetalias]
  > myparents(\$1) = parents(\$1)
  > EOF
  $ hg log --template '{rev} Parents: {revset("myparents(%s)", rev)}\n'
  2 Parents: 1
  1 Parents: 0
  0 Parents: 

  $ hg log --template 'Rev: {rev}\n{revset("::%s", rev) % "Ancestor: {revision}\n"}\n'
  Rev: 2
  Ancestor: 0
  Ancestor: 1
  Ancestor: 2
  
  Rev: 1
  Ancestor: 0
  Ancestor: 1
  
  Rev: 0
  Ancestor: 0
  
  $ hg log --template '{revset("TIP"|lower)}\n' -l1
  2

  $ hg log -T '{revset("%s", "t{"ip"}")}\n' -l1
  2

 a list template is evaluated for each item of revset/parents

  $ hg log -T '{rev} p: {revset("p1(%s)", rev) % "{rev}:{node|short}"}\n'
  2 p: 1:ea4c0948489d
  1 p: 0:f7769ec2ab97
  0 p: 

  $ hg log --debug -T '{rev} p:{parents % " {rev}:{node|short}"}\n'
  2 p: 1:ea4c0948489d -1:000000000000
  1 p: 0:f7769ec2ab97 -1:000000000000
  0 p: -1:000000000000 -1:000000000000

 therefore, 'revcache' should be recreated for each rev

  $ hg log -T '{rev} {file_adds}\np {revset("p1(%s)", rev) % "{file_adds}"}\n'
  2 aa b
  p 
  1 
  p a
  0 a
  p 

  $ hg log --debug -T '{rev} {file_adds}\np {parents % "{file_adds}"}\n'
  2 aa b
  p 
  1 
  p a
  0 a
  p 

a revset item must be evaluated as an integer revision, not an offset from tip

  $ hg log -l 1 -T '{revset("null") % "{rev}:{node|short}"}\n'
  -1:000000000000
  $ hg log -l 1 -T '{revset("%s", "null") % "{rev}:{node|short}"}\n'
  -1:000000000000

join() should pick '{rev}' from revset items:

  $ hg log -R ../a -T '{join(revset("parents(%d)", rev), ", ")}\n' -r6
  4, 5

on the other hand, parents are formatted as '{rev}:{node|formatnode}' by
default. join() should agree with the default formatting:

  $ hg log -R ../a -T '{join(parents, ", ")}\n' -r6
  5:13207e5a10d9, 4:07fa1db10648

  $ hg log -R ../a -T '{join(parents, ",\n")}\n' -r6 --debug
  5:13207e5a10d9fd28ec424934298e176197f2c67f,
  4:07fa1db1064879a32157227401eb44b322ae53ce

Test files function

  $ hg log -T "{rev}\n{join(files('*'), '\n')}\n"
  2
  a
  aa
  b
  1
  a
  0
  a

  $ hg log -T "{rev}\n{join(files('aa'), '\n')}\n"
  2
  aa
  1
  
  0
  

Test relpath function

  $ hg log -r0 -T '{files % "{file|relpath}\n"}'
  a
  $ cd ..
  $ hg log -R r -r0 -T '{files % "{file|relpath}\n"}'
  r/a
  $ cd r

Test active bookmark templating

  $ hg book foo
  $ hg book bar
  $ hg log --template "{rev} {bookmarks % '{bookmark}{ifeq(bookmark, active, \"*\")} '}\n"
  2 bar* foo text.{rev} 
  1 
  0 
  $ hg log --template "{rev} {activebookmark}\n"
  2 bar
  1 
  0 
  $ hg bookmarks --inactive bar
  $ hg log --template "{rev} {activebookmark}\n"
  2 
  1 
  0 
  $ hg book -r1 baz
  $ hg log --template "{rev} {join(bookmarks, ' ')}\n"
  2 bar foo text.{rev}
  1 baz
  0 
  $ hg log --template "{rev} {ifcontains('foo', bookmarks, 't', 'f')}\n"
  2 t
  1 f
  0 f

Test namespaces dict

  $ hg --config extensions.revnamesext=$TESTDIR/revnamesext.py log -T '{rev}\n{namespaces % " {namespace} color={colorname} builtin={builtin}\n  {join(names, ",")}\n"}\n'
  2
   bookmarks color=bookmark builtin=True
    bar,foo,text.{rev}
   tags color=tag builtin=True
    tip
   branches color=branch builtin=True
    default
   revnames color=revname builtin=False
    r2
  
  1
   bookmarks color=bookmark builtin=True
    baz
   tags color=tag builtin=True
    
   branches color=branch builtin=True
    default
   revnames color=revname builtin=False
    r1
  
  0
   bookmarks color=bookmark builtin=True
    
   tags color=tag builtin=True
    
   branches color=branch builtin=True
    default
   revnames color=revname builtin=False
    r0
  
  $ hg log -r2 -T '{namespaces % "{namespace}: {names}\n"}'
  bookmarks: bar foo text.{rev}
  tags: tip
  branches: default
  $ hg log -r2 -T '{namespaces % "{namespace}:\n{names % " {name}\n"}"}'
  bookmarks:
   bar
   foo
   text.{rev}
  tags:
   tip
  branches:
   default
  $ hg log -r2 -T '{get(namespaces, "bookmarks") % "{name}\n"}'
  bar
  foo
  text.{rev}
  $ hg log -r2 -T '{namespaces.bookmarks % "{bookmark}\n"}'
  bar
  foo
  text.{rev}

Test stringify on sub expressions

  $ cd ..
  $ hg log -R a -r 8 --template '{join(files, if("1", if("1", ", ")))}\n'
  fourth, second, third
  $ hg log -R a -r 8 --template '{strip(if("1", if("1", "-abc-")), if("1", if("1", "-")))}\n'
  abc

Test splitlines

  $ hg log -Gv -R a --template "{splitlines(desc) % 'foo {line}\n'}"
  @  foo Modify, add, remove, rename
  |
  o  foo future
  |
  o  foo third
  |
  o  foo second
  
  o    foo merge
  |\
  | o  foo new head
  | |
  o |  foo new branch
  |/
  o  foo no user, no domain
  |
  o  foo no person
  |
  o  foo other 1
  |  foo other 2
  |  foo
  |  foo other 3
  o  foo line 1
     foo line 2

  $ hg log -R a -r0 -T '{desc|splitlines}\n'
  line 1 line 2
  $ hg log -R a -r0 -T '{join(desc|splitlines, "|")}\n'
  line 1|line 2

Test startswith
  $ hg log -Gv -R a --template "{startswith(desc)}"
  hg: parse error: startswith expects two arguments
  [255]

  $ hg log -Gv -R a --template "{startswith('line', desc)}"
  @
  |
  o
  |
  o
  |
  o
  
  o
  |\
  | o
  | |
  o |
  |/
  o
  |
  o
  |
  o
  |
  o  line 1
     line 2

Test bad template with better error message

  $ hg log -Gv -R a --template '{desc|user()}'
  hg: parse error: expected a symbol, got 'func'
  [255]

Test word function (including index out of bounds graceful failure)

  $ hg log -Gv -R a --template "{word('1', desc)}"
  @  add,
  |
  o
  |
  o
  |
  o
  
  o
  |\
  | o  head
  | |
  o |  branch
  |/
  o  user,
  |
  o  person
  |
  o  1
  |
  o  1
  

Test word third parameter used as splitter

  $ hg log -Gv -R a --template "{word('0', desc, 'o')}"
  @  M
  |
  o  future
  |
  o  third
  |
  o  sec
  
  o    merge
  |\
  | o  new head
  | |
  o |  new branch
  |/
  o  n
  |
  o  n
  |
  o
  |
  o  line 1
     line 2

Test word error messages for not enough and too many arguments

  $ hg log -Gv -R a --template "{word('0')}"
  hg: parse error: word expects two or three arguments, got 1
  [255]

  $ hg log -Gv -R a --template "{word('0', desc, 'o', 'h', 'b', 'o', 'y')}"
  hg: parse error: word expects two or three arguments, got 7
  [255]

Test word for integer literal

  $ hg log -R a --template "{word(2, desc)}\n" -r0
  line

Test word for invalid numbers

  $ hg log -Gv -R a --template "{word('a', desc)}"
  hg: parse error: word expects an integer index
  [255]

Test word for out of range

  $ hg log -R a --template "{word(10000, desc)}"
  $ hg log -R a --template "{word(-10000, desc)}"

Test indent and not adding to empty lines

  $ hg log -T "-----\n{indent(desc, '>> ', ' > ')}\n" -r 0:1 -R a
  -----
   > line 1
  >> line 2
  -----
   > other 1
  >> other 2
  
  >> other 3

Test with non-strings like dates

  $ hg log -T "{indent(date, '   ')}\n" -r 2:3 -R a
     1200000.00
     1300000.00

Test broken string escapes:

  $ hg log -T "bogus\\" -R a
  hg: parse error: trailing \ in string
  [255]
  $ hg log -T "\\xy" -R a
  hg: parse error: invalid \x escape
  [255]

json filter should escape HTML tags so that the output can be embedded in hgweb:

  $ hg log -T "{'<foo@example.org>'|json}\n" -R a -l1
  "\u003cfoo@example.org\u003e"

Templater supports aliases of symbol and func() styles:

  $ hg clone -q a aliases
  $ cd aliases
  $ cat <<EOF >> .hg/hgrc
  > [templatealias]
  > r = rev
  > rn = "{r}:{node|short}"
  > status(c, files) = files % "{c} {file}\n"
  > utcdate(d) = localdate(d, "UTC")
  > EOF

  $ hg debugtemplate -vr0 '{rn} {utcdate(date)|isodate}\n'
  (template
    (symbol 'rn')
    (string ' ')
    (|
      (func
        (symbol 'utcdate')
        (symbol 'date'))
      (symbol 'isodate'))
    (string '\n'))
  * expanded:
  (template
    (template
      (symbol 'rev')
      (string ':')
      (|
        (symbol 'node')
        (symbol 'short')))
    (string ' ')
    (|
      (func
        (symbol 'localdate')
        (list
          (symbol 'date')
          (string 'UTC')))
      (symbol 'isodate'))
    (string '\n'))
  0:1e4e1b8f71e0 1970-01-12 13:46 +0000

  $ hg debugtemplate -vr0 '{status("A", file_adds)}'
  (template
    (func
      (symbol 'status')
      (list
        (string 'A')
        (symbol 'file_adds'))))
  * expanded:
  (template
    (%
      (symbol 'file_adds')
      (template
        (string 'A')
        (string ' ')
        (symbol 'file')
        (string '\n'))))
  A a

A unary function alias can be called as a filter:

  $ hg debugtemplate -vr0 '{date|utcdate|isodate}\n'
  (template
    (|
      (|
        (symbol 'date')
        (symbol 'utcdate'))
      (symbol 'isodate'))
    (string '\n'))
  * expanded:
  (template
    (|
      (func
        (symbol 'localdate')
        (list
          (symbol 'date')
          (string 'UTC')))
      (symbol 'isodate'))
    (string '\n'))
  1970-01-12 13:46 +0000

Aliases should be applied only to command arguments and templates in hgrc.
Otherwise, our stock styles and web templates could be corrupted:

  $ hg log -r0 -T '{rn} {utcdate(date)|isodate}\n'
  0:1e4e1b8f71e0 1970-01-12 13:46 +0000

  $ hg log -r0 --config ui.logtemplate='"{rn} {utcdate(date)|isodate}\n"'
  0:1e4e1b8f71e0 1970-01-12 13:46 +0000

  $ cat <<EOF > tmpl
  > changeset = 'nothing expanded:{rn}\n'
  > EOF
  $ hg log -r0 --style ./tmpl
  nothing expanded:

Aliases in formatter:

  $ hg bookmarks -T '{pad(bookmark, 7)} {rn}\n'
  foo     4:07fa1db10648

Aliases should honor HGPLAIN:

  $ HGPLAIN= hg log -r0 -T 'nothing expanded:{rn}\n'
  nothing expanded:
  $ HGPLAINEXCEPT=templatealias hg log -r0 -T '{rn}\n'
  0:1e4e1b8f71e0

Unparsable alias:

  $ hg debugtemplate --config templatealias.bad='x(' -v '{bad}'
  (template
    (symbol 'bad'))
  abort: bad definition of template alias "bad": at 2: not a prefix: end
  [255]
  $ hg log --config templatealias.bad='x(' -T '{bad}'
  abort: bad definition of template alias "bad": at 2: not a prefix: end
  [255]

  $ cd ..

Set up repository for non-ascii encoding tests:

  $ hg init nonascii
  $ cd nonascii
  $ $PYTHON <<EOF
  > open('latin1', 'w').write('\xe9')
  > open('utf-8', 'w').write('\xc3\xa9')
  > EOF
  $ HGENCODING=utf-8 hg bookmark -q `cat utf-8`
  $ HGENCODING=utf-8 hg ci -qAm "non-ascii branch: `cat utf-8`" utf-8

json filter should try round-trip conversion to utf-8:

  $ HGENCODING=ascii hg log -T "{bookmarks|json}\n" -r0
  ["\u00e9"]
  $ HGENCODING=ascii hg log -T "{desc|json}\n" -r0
  "non-ascii branch: \u00e9"

json filter takes input as utf-8b:

  $ HGENCODING=ascii hg log -T "{'`cat utf-8`'|json}\n" -l1
  "\u00e9"
  $ HGENCODING=ascii hg log -T "{'`cat latin1`'|json}\n" -l1
  "\udce9"

utf8 filter:

  $ HGENCODING=ascii hg log -T "round-trip: {bookmarks % '{bookmark|utf8|hex}'}\n" -r0
  round-trip: c3a9
  $ HGENCODING=latin1 hg log -T "decoded: {'`cat latin1`'|utf8|hex}\n" -l1
  decoded: c3a9
  $ HGENCODING=ascii hg log -T "replaced: {'`cat latin1`'|utf8|hex}\n" -l1
  abort: decoding near * (glob)
  [255]
  $ hg log -T "invalid type: {rev|utf8}\n" -r0
  abort: template filter 'utf8' is not compatible with keyword 'rev'
  [255]

pad width:

  $ HGENCODING=utf-8 hg debugtemplate "{pad('`cat utf-8`', 2, '-')}\n"
  \xc3\xa9- (esc)

  $ cd ..

Test that template function in extension is registered as expected

  $ cd a

  $ cat <<EOF > $TESTTMP/customfunc.py
  > from edenscm.mercurial import registrar
  > 
  > templatefunc = registrar.templatefunc()
  > 
  > @templatefunc('custom()')
  > def custom(context, mapping, args):
  >     return 'custom'
  > EOF
  $ cat <<EOF > .hg/hgrc
  > [extensions]
  > customfunc = $TESTTMP/customfunc.py
  > EOF

  $ hg log -r . -T "{custom()}\n" --config customfunc.enabled=true
  custom

  $ cd ..

Test 'graphwidth' in 'hg log' on various topologies. The key here is that the
printed graphwidths 3, 5, 7, etc. should all line up in their respective
columns. We don't care about other aspects of the graph rendering here.

  $ hg init graphwidth
  $ cd graphwidth

  $ wrappabletext="a a a a a a a a a a a a"

  $ printf "first\n" > file
  $ hg add file
  $ hg commit -m "$wrappabletext"

  $ printf "first\nsecond\n" > file
  $ hg commit -m "$wrappabletext"

  $ hg checkout 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ printf "third\nfirst\n" > file
  $ hg commit -m "$wrappabletext"

  $ hg merge
  merging file
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg log --graph -T "{graphwidth}"
  @  3
  |
  | @  5
  |/
  o  3
  
  $ hg commit -m "$wrappabletext"

  $ hg log --graph -T "{graphwidth}"
  @    5
  |\
  | o  5
  | |
  o |  5
  |/
  o  3
  

  $ hg checkout 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ printf "third\nfirst\nsecond\n" > file
  $ hg commit -m "$wrappabletext"

  $ hg log --graph -T "{graphwidth}"
  @  3
  |
  | o    7
  | |\
  +---o  7
  | |
  | o  5
  |/
  o  3
  

  $ hg log --graph -T "{graphwidth}" -r 3
  o    5
  |\
  ~ ~

  $ hg log --graph -T "{graphwidth}" -r 1
  o  3
  |
  ~

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "$wrappabletext"

  $ printf "seventh\n" >> file
  $ hg commit -m "$wrappabletext"

  $ hg log --graph -T "{graphwidth}"
  @  3
  |
  o    5
  |\
  | o  5
  | |
  o |    7
  |\ \
  | o |  7
  | |/
  o /  5
  |/
  o  3
  

The point of graphwidth is to allow wrapping that accounts for the space taken
by the graph.

  $ COLUMNS=10 hg log --graph -T "{fill(desc, termwidth - graphwidth)}"
  @  a a a a
  |  a a a a
  |  a a a a
  o    a a a
  |\   a a a
  | |  a a a
  | |  a a a
  | o  a a a
  | |  a a a
  | |  a a a
  | |  a a a
  o |    a a
  |\ \   a a
  | | |  a a
  | | |  a a
  | | |  a a
  | | |  a a
  | o |  a a
  | |/   a a
  | |    a a
  | |    a a
  | |    a a
  | |    a a
  o |  a a a
  |/   a a a
  |    a a a
  |    a a a
  o  a a a a
     a a a a
     a a a a

Something tricky happens when there are elided nodes; the next drawn row of
edges can be more than one column wider, but the graph width only increases by
one column. The remaining columns are added in between the nodes.

  $ hg log --graph -T "{graphwidth}" -r "0|2|4|5"
  o    5
  |\
  | \
  | :\
  o : :  7
  :/ /
  : o  5
  :/
  o  3
  

  $ cd ..

Confirm that truncation does the right thing

  $ hg debugtemplate '{truncatelonglines("abcdefghijklmnopqrst\n", 10)}'
  abcdefghij
  $ hg debugtemplate '{truncatelonglines("abcdefghijklmnopqrst\n", 10, "…")}'
  abcdefghi\xe2\x80\xa6 (esc)
  $ hg debugtemplate '{truncate("a\nb\nc\n", 2)}'
  a
  b
  $ hg debugtemplate '{truncate("a\nb\nc\n", 2, "truncated\n")}'
  a
  truncated
