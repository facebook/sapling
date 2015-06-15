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

  $ echo foo > .hg/branch
  $ hg commit -m 'new branch' -d '1400000 0' -u 'person'

  $ hg co -q 3
  $ echo other 4 >> d
  $ hg add d
  $ hg commit -m 'new head' -d '1500000 0' -u 'person'

  $ hg merge -q foo
  $ hg commit -m 'merge' -d '1500001 0' -u 'person'

Second branch starting at nullrev:

  $ hg update null
  0 files updated, 0 files merged, 4 files removed, 0 files unresolved
  $ echo second > second
  $ hg add second
  $ hg commit -m second -d '1000000 0' -u 'User Name <user@hostname>'
  created new head

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

Make sure user/global hgrc does not affect tests

  $ echo '[ui]' > .hg/hgrc
  $ echo 'logtemplate =' >> .hg/hgrc
  $ echo 'style =' >> .hg/hgrc

Add some simple styles to settings

  $ echo '[templates]' >> .hg/hgrc
  $ printf 'simple = "{rev}\\n"\n' >> .hg/hgrc
  $ printf 'simple2 = {rev}\\n\n' >> .hg/hgrc

  $ hg log -l1 -Tsimple
  8
  $ hg log -l1 -Tsimple2
  8

Test templates and style maps in files:

  $ echo "{rev}" > tmpl
  $ hg log -l1 -T./tmpl
  8
  $ hg log -l1 -Tblah/blah
  blah/blah (no-eol)

  $ printf 'changeset = "{rev}\\n"\n' > map-simple
  $ hg log -l1 -T./map-simple
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
  $ diff -U 0 log.out phases.out | grep -v '^---\|^+++'
  @@ -2,0 +3 @@
  +phase:       draft
  @@ -6,0 +8 @@
  +phase:       draft
  @@ -11,0 +14 @@
  +phase:       draft
  @@ -17,0 +21 @@
  +phase:       draft
  @@ -24,0 +29 @@
  +phase:       draft
  @@ -31,0 +37 @@
  +phase:       draft
  @@ -36,0 +43 @@
  +phase:       draft
  @@ -41,0 +49 @@
  +phase:       draft
  @@ -46,0 +55 @@
  +phase:       draft
  @@ -51,0 +61 @@
  +phase:       draft

  $ hg log -v > log.out
  $ hg log -v --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg log -v -T phases > phases.out
  $ diff -U 0 log.out phases.out | grep -v '^---\|^+++'
  @@ -2,0 +3 @@
  +phase:       draft
  @@ -7,0 +9 @@
  +phase:       draft
  @@ -15,0 +18 @@
  +phase:       draft
  @@ -24,0 +28 @@
  +phase:       draft
  @@ -33,0 +38 @@
  +phase:       draft
  @@ -43,0 +49 @@
  +phase:       draft
  @@ -50,0 +57 @@
  +phase:       draft
  @@ -58,0 +66 @@
  +phase:       draft
  @@ -66,0 +75 @@
  +phase:       draft
  @@ -77,0 +87 @@
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
  $ diff -U 0 log.out phases.out | grep -v '^---\|^+++'
  @@ -2,0 +3 @@
  +[log.phase|phase:       draft]
  @@ -6,0 +8 @@
  +[log.phase|phase:       draft]
  @@ -11,0 +14 @@
  +[log.phase|phase:       draft]
  @@ -17,0 +21 @@
  +[log.phase|phase:       draft]
  @@ -24,0 +29 @@
  +[log.phase|phase:       draft]
  @@ -31,0 +37 @@
  +[log.phase|phase:       draft]
  @@ -36,0 +43 @@
  +[log.phase|phase:       draft]
  @@ -41,0 +49 @@
  +[log.phase|phase:       draft]
  @@ -46,0 +55 @@
  +[log.phase|phase:       draft]
  @@ -51,0 +61 @@
  +[log.phase|phase:       draft]

  $ hg --color=debug -v log > log.out
  $ hg --color=debug -v log --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out
  $ hg --color=debug -v log -T phases > phases.out
  $ diff -U 0 log.out phases.out | grep -v '^---\|^+++'
  @@ -2,0 +3 @@
  +[log.phase|phase:       draft]
  @@ -7,0 +9 @@
  +[log.phase|phase:       draft]
  @@ -15,0 +18 @@
  +[log.phase|phase:       draft]
  @@ -24,0 +28 @@
  +[log.phase|phase:       draft]
  @@ -33,0 +38 @@
  +[log.phase|phase:       draft]
  @@ -43,0 +49 @@
  +[log.phase|phase:       draft]
  @@ -50,0 +57 @@
  +[log.phase|phase:       draft]
  @@ -58,0 +66 @@
  +[log.phase|phase:       draft]
  @@ -66,0 +75 @@
  +[log.phase|phase:       draft]
  @@ -77,0 +87 @@
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

  $ hg --config extensions.strip= strip -q .

Revision with no copies (used to print a traceback):

  $ hg tip -v --template '\n'
  

Compact style works:

  $ hg log -Tcompact
  8[tip]   95c24699272e   2020-01-01 10:01 +0000   test
    third
  
  7:-1   29114dbae42b   1970-01-12 13:46 +0000   user
    second
  
  6:5,4   d41e714fe50d   1970-01-18 08:40 +0000   person
    merge
  
  5:3   13207e5a10d9   1970-01-18 08:40 +0000   person
    new head
  
  4   bbe44766e73d   1970-01-17 04:53 +0000   person
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
  
  6:5,4   d41e714fe50d   1970-01-18 08:40 +0000   person
    merge
  
  5:3   13207e5a10d9   1970-01-18 08:40 +0000   person
    new head
  
  4   bbe44766e73d   1970-01-17 04:53 +0000   person
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
  
  6:5,4   d41e714fe50d   1970-01-18 08:40 +0000   person
    merge
  
  5:3,-1   13207e5a10d9   1970-01-18 08:40 +0000   person
    new head
  
  4:3,-1   bbe44766e73d   1970-01-17 04:53 +0000   person
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
  <logentry revision="6" node="d41e714fe50d9e4a5f11b4d595d543481b5f980b">
  <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
  <parent revision="4" node="bbe44766e73d5f11ed2177f1838de10c53ef3e74" />
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
  <logentry revision="4" node="bbe44766e73d5f11ed2177f1838de10c53ef3e74">
  <branch>foo</branch>
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
  <logentry revision="6" node="d41e714fe50d9e4a5f11b4d595d543481b5f980b">
  <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
  <parent revision="4" node="bbe44766e73d5f11ed2177f1838de10c53ef3e74" />
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
  <logentry revision="4" node="bbe44766e73d5f11ed2177f1838de10c53ef3e74">
  <branch>foo</branch>
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
  <logentry revision="6" node="d41e714fe50d9e4a5f11b4d595d543481b5f980b">
  <parent revision="5" node="13207e5a10d9fd28ec424934298e176197f2c67f" />
  <parent revision="4" node="bbe44766e73d5f11ed2177f1838de10c53ef3e74" />
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
  <logentry revision="4" node="bbe44766e73d5f11ed2177f1838de10c53ef3e74">
  <branch>foo</branch>
  <parent revision="3" node="10e46f2dcbf4823578cf180f33ecf0b957964c47" />
  <parent revision="-1" node="0000000000000000000000000000000000000000" />
  <author email="person">person</author>
  <date>1970-01-17T04:53:20+00:00</date>
  <msg xml:space="preserve">new branch</msg>
  <paths>
  </paths>
  <extra key="branch">foo</extra>
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
    "node": "d41e714fe50d9e4a5f11b4d595d543481b5f980b",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500001, 0],
    "desc": "merge",
    "bookmarks": [],
    "tags": [],
    "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "bbe44766e73d5f11ed2177f1838de10c53ef3e74"]
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
    "node": "bbe44766e73d5f11ed2177f1838de10c53ef3e74",
    "branch": "foo",
    "phase": "draft",
    "user": "person",
    "date": [1400000, 0],
    "desc": "new branch",
    "bookmarks": [],
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
    "node": "d41e714fe50d9e4a5f11b4d595d543481b5f980b",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500001, 0],
    "desc": "merge",
    "bookmarks": [],
    "tags": [],
    "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "bbe44766e73d5f11ed2177f1838de10c53ef3e74"],
    "files": []
   },
   {
    "rev": 4,
    "node": "bbe44766e73d5f11ed2177f1838de10c53ef3e74",
    "branch": "foo",
    "phase": "draft",
    "user": "person",
    "date": [1400000, 0],
    "desc": "new branch",
    "bookmarks": [],
    "tags": [],
    "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"],
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
    "node": "d41e714fe50d9e4a5f11b4d595d543481b5f980b",
    "branch": "default",
    "phase": "draft",
    "user": "person",
    "date": [1500001, 0],
    "desc": "merge",
    "bookmarks": [],
    "tags": [],
    "parents": ["13207e5a10d9fd28ec424934298e176197f2c67f", "bbe44766e73d5f11ed2177f1838de10c53ef3e74"],
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
    "node": "bbe44766e73d5f11ed2177f1838de10c53ef3e74",
    "branch": "foo",
    "phase": "draft",
    "user": "person",
    "date": [1400000, 0],
    "desc": "new branch",
    "bookmarks": [],
    "tags": [],
    "parents": ["10e46f2dcbf4823578cf180f33ecf0b957964c47"],
    "manifest": "cb5a1327723bada42f117e4c55a303246eaf9ccc",
    "extra": {"branch": "foo"},
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
  (available styles: bisect, changelog, compact, default, phases, status, xml)
  [255]

  $ hg log -T list
  available styles: bisect, changelog, compact, default, phases, status, xml
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
  abort: t:1: missing value
  [255]

Error if include fails:

  $ echo 'changeset = q' >> t
#if unix-permissions no-root
  $ hg log --style ./t
  abort: template file ./q: Permission denied
  [255]
  $ rm q
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
  	[d41e714fe50d]
  
  	* d:
  	new head
  	[13207e5a10d9]
  
  1970-01-17  person  <person>
  
  	* new branch
  	[bbe44766e73d] <foo>
  
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
  	[d41e714fe50d]
  
  1970-01-17  person  <person>
  
  	* new branch
  	[bbe44766e73d] <foo>
  

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
  branch: foo
  branch: default
  branch: default
  branch: default
  branch: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: foo
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--verbose: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: foo
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branch--debug: default
  branches: 
  branches: 
  branches: 
  branches: 
  branches: foo
  branches: 
  branches: 
  branches: 
  branches: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: foo
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--verbose: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: 
  branches--debug: foo
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
  node: d41e714fe50d9e4a5f11b4d595d543481b5f980b
  node: 13207e5a10d9fd28ec424934298e176197f2c67f
  node: bbe44766e73d5f11ed2177f1838de10c53ef3e74
  node: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  node: 97054abb4ab824450e9164180baf491ae0078465
  node: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  node: 1e4e1b8f71e05681d422154f5421e385fec3454f
  node--verbose: 95c24699272ef57d062b8bccc32c878bf841784a
  node--verbose: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  node--verbose: d41e714fe50d9e4a5f11b4d595d543481b5f980b
  node--verbose: 13207e5a10d9fd28ec424934298e176197f2c67f
  node--verbose: bbe44766e73d5f11ed2177f1838de10c53ef3e74
  node--verbose: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  node--verbose: 97054abb4ab824450e9164180baf491ae0078465
  node--verbose: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  node--verbose: 1e4e1b8f71e05681d422154f5421e385fec3454f
  node--debug: 95c24699272ef57d062b8bccc32c878bf841784a
  node--debug: 29114dbae42b9f078cf2714dbe3a86bba8ec7453
  node--debug: d41e714fe50d9e4a5f11b4d595d543481b5f980b
  node--debug: 13207e5a10d9fd28ec424934298e176197f2c67f
  node--debug: bbe44766e73d5f11ed2177f1838de10c53ef3e74
  node--debug: 10e46f2dcbf4823578cf180f33ecf0b957964c47
  node--debug: 97054abb4ab824450e9164180baf491ae0078465
  node--debug: b608e9d1a3f0273ccf70fb85fd6866b3482bf965
  node--debug: 1e4e1b8f71e05681d422154f5421e385fec3454f
  parents: 
  parents: -1:000000000000 
  parents: 5:13207e5a10d9 4:bbe44766e73d 
  parents: 3:10e46f2dcbf4 
  parents: 
  parents: 
  parents: 
  parents: 
  parents: 
  parents--verbose: 
  parents--verbose: -1:000000000000 
  parents--verbose: 5:13207e5a10d9 4:bbe44766e73d 
  parents--verbose: 3:10e46f2dcbf4 
  parents--verbose: 
  parents--verbose: 
  parents--verbose: 
  parents--verbose: 
  parents--verbose: 
  parents--debug: 7:29114dbae42b9f078cf2714dbe3a86bba8ec7453 -1:0000000000000000000000000000000000000000 
  parents--debug: -1:0000000000000000000000000000000000000000 -1:0000000000000000000000000000000000000000 
  parents--debug: 5:13207e5a10d9fd28ec424934298e176197f2c67f 4:bbe44766e73d5f11ed2177f1838de10c53ef3e74 
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
  extras: branch=foo
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=foo
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--verbose: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=default
  extras--debug: branch=foo
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
  p2node: bbe44766e73d5f11ed2177f1838de10c53ef3e74
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: bbe44766e73d5f11ed2177f1838de10c53ef3e74
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--verbose: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: 0000000000000000000000000000000000000000
  p2node--debug: bbe44766e73d5f11ed2177f1838de10c53ef3e74
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
  d41e714fe50d
  13207e5a10d9
  bbe44766e73d
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
  5: 6:d41e714fe50d
  4: 6:d41e714fe50d
  3: 4:bbe44766e73d 5:13207e5a10d9
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

  >>> from datetime import datetime, timedelta
  >>> fp = open('a', 'w')
  >>> n = datetime.now() + timedelta(366 * 7)
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

  $ hg log -r0 --template '{branch|upper}\n'
  DEFAULT
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
  manifest:    8:89dd546f2de0a9d6d664f58d86097eb97baba567
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
  [ui.debug log.manifest|manifest:    8:89dd546f2de0a9d6d664f58d86097eb97baba567]
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
  
  changeset:   4:bbe44766e73d
  bisect:      bad (implicit)
  branch:      foo
  user:        person
  date:        Sat Jan 17 04:53:20 1970 +0000
  summary:     new branch
  
  $ hg log --debug -T bisect -r 0:4
  changeset:   0:1e4e1b8f71e05681d422154f5421e385fec3454f
  bisect:      good (implicit)
  phase:       public
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    0:a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
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
  manifest:    1:4e8d705b1e53e3f9375e0e60dc7b525d8211fe55
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
  manifest:    2:6e0e82995c35d0d57a52aca8da4e56139e06b4b1
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
  manifest:    3:cb5a1327723bada42f117e4c55a303246eaf9ccc
  user:        person
  date:        Fri Jan 16 01:06:40 1970 +0000
  files:       c
  extra:       branch=default
  description:
  no user, no domain
  
  
  changeset:   4:bbe44766e73d5f11ed2177f1838de10c53ef3e74
  bisect:      bad (implicit)
  branch:      foo
  phase:       draft
  parent:      3:10e46f2dcbf4823578cf180f33ecf0b957964c47
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    3:cb5a1327723bada42f117e4c55a303246eaf9ccc
  user:        person
  date:        Sat Jan 17 04:53:20 1970 +0000
  extra:       branch=foo
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
  
  
  changeset:   4:bbe44766e73d
  bisect:      bad (implicit)
  branch:      foo
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
  
  [log.changeset changeset.draft|changeset:   4:bbe44766e73d]
  [log.bisect bisect.bad|bisect:      bad (implicit)]
  [log.branch|branch:      foo]
  [log.user|user:        person]
  [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
  [log.summary|summary:     new branch]
  
  $ hg --color=debug log --debug -T bisect -r 0:4
  [log.changeset changeset.public|changeset:   0:1e4e1b8f71e05681d422154f5421e385fec3454f]
  [log.bisect bisect.good|bisect:      good (implicit)]
  [log.phase|phase:       public]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    0:a0c8bcbbb45c63b90b70ad007bf38961f64f2af0]
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
  [ui.debug log.manifest|manifest:    1:4e8d705b1e53e3f9375e0e60dc7b525d8211fe55]
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
  [ui.debug log.manifest|manifest:    2:6e0e82995c35d0d57a52aca8da4e56139e06b4b1]
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
  [ui.debug log.manifest|manifest:    3:cb5a1327723bada42f117e4c55a303246eaf9ccc]
  [log.user|user:        person]
  [log.date|date:        Fri Jan 16 01:06:40 1970 +0000]
  [ui.debug log.files|files:       c]
  [ui.debug log.extra|extra:       branch=default]
  [ui.note log.description|description:]
  [ui.note log.description|no user, no domain]
  
  
  [log.changeset changeset.draft|changeset:   4:bbe44766e73d5f11ed2177f1838de10c53ef3e74]
  [log.bisect bisect.bad|bisect:      bad (implicit)]
  [log.branch|branch:      foo]
  [log.phase|phase:       draft]
  [log.parent changeset.public|parent:      3:10e46f2dcbf4823578cf180f33ecf0b957964c47]
  [log.parent changeset.public|parent:      -1:0000000000000000000000000000000000000000]
  [ui.debug log.manifest|manifest:    3:cb5a1327723bada42f117e4c55a303246eaf9ccc]
  [log.user|user:        person]
  [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
  [ui.debug log.extra|extra:       branch=foo]
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
  
  
  [log.changeset changeset.draft|changeset:   4:bbe44766e73d]
  [log.bisect bisect.bad|bisect:      bad (implicit)]
  [log.branch|branch:      foo]
  [log.user|user:        person]
  [log.date|date:        Sat Jan 17 04:53:20 1970 +0000]
  [ui.note log.description|description:]
  [ui.note log.description|new branch]
  
  
  $ hg bisect --reset

Error on syntax:

  $ echo 'x = "f' >> t
  $ hg log
  abort: t:3: unmatched quotes
  [255]

  $ hg log -T '{date'
  hg: parse error at 1: unterminated template expansion
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

Behind the scenes, this will throw ValueError

  $ hg tip --template '{author|email|date}\n'
  abort: template filter 'datefilter' is not compatible with keyword 'author'
  [255]

Error in nested template:

  $ hg log -T '{"date'
  hg: parse error at 2: unterminated string
  [255]

  $ hg log -T '{"foo{date|=}"}'
  hg: parse error at 11: syntax error
  [255]

Thrown an error if a template function doesn't exist

  $ hg tip --template '{foo()}\n'
  hg: parse error: unknown function 'foo'
  [255]

Pass generator object created by template function to filter

  $ hg log -l 1 --template '{if(author, author)|user}\n'
  test

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
  created new head

  $ echo e >> head2
  $ hg ci -m h2e -d '4 0'

  $ hg merge -q
  $ hg ci -m merge -d '5 -3600'

No tag set:

  $ hg log --template '{rev}: {latesttag}+{latesttagdistance}\n'
  5: null+5
  4: null+4
  3: null+3
  2: null+3
  1: null+2
  0: null+1

One common tag: longest path wins:

  $ hg tag -r 1 -m t1 -d '6 0' t1
  $ hg log --template '{rev}: {latesttag}+{latesttagdistance}\n'
  6: t1+4
  5: t1+3
  4: t1+2
  3: t1+1
  2: t1+1
  1: t1+0
  0: null+1

One ancestor tag: more recent wins:

  $ hg tag -r 2 -m t2 -d '7 0' t2
  $ hg log --template '{rev}: {latesttag}+{latesttagdistance}\n'
  7: t2+3
  6: t2+2
  5: t2+1
  4: t1+2
  3: t1+1
  2: t2+0
  1: t1+0
  0: null+1

Two branch tags: more recent wins:

  $ hg tag -r 3 -m t3 -d '8 0' t3
  $ hg log --template '{rev}: {latesttag}+{latesttagdistance}\n'
  8: t3+5
  7: t3+4
  6: t3+3
  5: t3+2
  4: t3+1
  3: t3+0
  2: t2+0
  1: t1+0
  0: null+1

Merged tag overrides:

  $ hg tag -r 5 -m t5 -d '9 0' t5
  $ hg tag -r 3 -m at3 -d '10 0' at3
  $ hg log --template '{rev}: {latesttag}+{latesttagdistance}\n'
  10: t5+5
  9: t5+4
  8: t5+3
  7: t5+2
  6: t5+1
  5: t5+0
  4: at3:t3+1
  3: at3:t3+0
  2: t2+0
  1: t1+0
  0: null+1

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
  test 10:9b4a630e5f5f

Test recursive showlist template (issue1989):

  $ cat > style1989 <<EOF
  > changeset = '{file_mods}{manifest}{extras}'
  > file_mod  = 'M|{author|person}\n'
  > manifest = '{rev},{author}\n'
  > extra = '{key}: {author}\n'
  > EOF

  $ hg -R latesttag log -r tip --style=style1989
  M|test
  10,test
  branch: test

Test new-style inline templating:

  $ hg log -R latesttag -r tip --template 'modified files: {file_mods % " {file}\n"}\n'
  modified files:  .hgtags
  
Test the sub function of templating for expansion:

  $ hg log -R latesttag -r 10 --template '{sub("[0-9]", "x", "{rev}")}\n'
  xx

Test the strip function with chars specified:

  $ hg log -R latesttag --template '{desc}\n'
  at3
  t5
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

  $ hg log -Ra -r0 -T '{(0)}\n'
  0
  $ hg log -Ra -r0 -T '{(123)}\n'
  123
  $ hg log -Ra -r0 -T '{(-4)}\n'
  -4
  $ hg log -Ra -r0 -T '{(-)}\n'
  hg: parse error at 2: integer literal without digits
  [255]
  $ hg log -Ra -r0 -T '{(-a)}\n'
  hg: parse error at 2: integer literal without digits
  [255]

top-level integer literal is interpreted as symbol (i.e. variable name):

  $ hg log -Ra -r0 -T '{1}\n'
  
  $ hg log -Ra -r0 -T '{if("t", "{1}")}\n'
  
  $ hg log -Ra -r0 -T '{1|stringify}\n'
  

unless explicit symbol is expected:

  $ hg log -Ra -r0 -T '{desc|1}\n'
  hg: parse error: expected a symbol, got 'integer'
  [255]
  $ hg log -Ra -r0 -T '{1()}\n'
  hg: parse error: expected a symbol, got 'integer'
  [255]

Test string literal:

  $ hg log -Ra -r0 -T '{"string with no template fragment"}\n'
  string with no template fragment
  $ hg log -Ra -r0 -T '{"template: {rev}"}\n'
  template: 0
  $ hg log -Ra -r0 -T '{r"rawstring: {rev}"}\n'
  rawstring: {rev}

because map operation requires template, raw string can't be used

  $ hg log -Ra -r0 -T '{files % r"rawstring"}\n'
  hg: parse error: expected template specifier
  [255]

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
  [255]
  $ hg log -r 2 -T '{if(rev, \"\\"")}\n'
  hg: parse error at 11: syntax error
  [255]
  $ hg log -r 2 -T '{if(rev, r\"\\"")}\n'
  hg: parse error at 12: syntax error
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
  4:\x5c\x786eew bra\x5c\x786ech

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

  $ hg branch -q 'text.{rev}'
  $ echo aa >> aa
  $ hg ci -u '{node|short}' -m 'desc to be wrapped desc to be wrapped'

  $ hg log -l1 --template '{fill(desc, "20", author, branch)}'
  {node|short}desc to
  text.{rev}be wrapped
  text.{rev}desc to be
  text.{rev}wrapped (no-eol)
  $ hg log -l1 --template '{fill(desc, "20", "{node|short}:", "text.{rev}:")}'
  bcc7ff960b8e:desc to
  text.1:be wrapped
  text.1:desc to be
  text.1:wrapped (no-eol)

  $ hg log -l 1 --template '{sub(r"[0-9]", "-", author)}'
  {node|short} (no-eol)
  $ hg log -l 1 --template '{sub(r"[0-9]", "-", "{node|short}")}'
  bcc-ff---b-e (no-eol)

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > color=
  > [color]
  > mode=ansi
  > text.{rev} = red
  > text.1 = green
  > EOF
  $ hg log --color=always -l 1 --template '{label(branch, "text\n")}'
  \x1b[0;31mtext\x1b[0m (esc)
  $ hg log --color=always -l 1 --template '{label("text.{rev}", "text\n")}'
  \x1b[0;32mtext\x1b[0m (esc)

Test branches inside if statement:

  $ hg log -r 0 --template '{if(branches, "yes", "no")}\n'
  no

Test get function:

  $ hg log -r 0 --template '{get(extras, "branch")}\n'
  default
  $ hg log -r 0 --template '{get(files, "should_fail")}\n'
  hg: parse error: get() expects a dict as first argument
  [255]

Test shortest(node) function:

  $ echo b > b
  $ hg ci -qAm b
  $ hg log --template '{shortest(node)}\n'
  e777
  bcc7
  f776
  $ hg log --template '{shortest(node, 10)}\n'
  e777603221
  bcc7ff960b
  f7769ec2ab

Test pad function

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

Test template string in pad function

  $ hg log -r 0 -T '{pad("\{{rev}}", 10)} {author|user}\n'
  {0}        test

  $ hg log -r 0 -T '{pad(r"\{rev}", 10)} {author|user}\n'
  \{rev}     test

Test ifcontains function

  $ hg log --template '{rev} {ifcontains(rev, "2 two 0", "is in the string", "is not")}\n'
  2 is in the string
  1 is not
  0 is in the string

  $ hg log --template '{rev} {ifcontains("a", file_adds, "added a", "did not add a")}\n'
  2 did not add a
  1 did not add a
  0 added a

Test revset function

  $ hg log --template '{rev} {ifcontains(rev, revset("."), "current rev", "not current rev")}\n'
  2 current rev
  1 not current rev
  0 not current rev

  $ hg log --template '{rev} {ifcontains(rev, revset(". + .^"), "match rev", "not match rev")}\n'
  2 match rev
  1 match rev
  0 not match rev

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

Test active bookmark templating

  $ hg book foo
  $ hg book bar
  $ hg log --template "{rev} {bookmarks % '{bookmark}{ifeq(bookmark, active, \"*\")} '}\n"
  2 bar* foo 
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
  2 bar foo
  1 baz
  0 
  $ hg log --template "{rev} {ifcontains('foo', bookmarks, 't', 'f')}\n"
  2 t
  1 f
  0 f

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
