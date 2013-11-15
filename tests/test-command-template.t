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
  $ hg log --template '{file_copies % "{source} -> {name}\n"}' -r .
  second -> fourth

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

Default style is like normal output:

  $ hg log > log.out
  $ hg log --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out

  $ hg log -v > log.out
  $ hg log -v --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out

  $ hg log --debug > log.out
  $ hg log --debug --style default > style.out
  $ cmp log.out style.out || diff -u log.out style.out

Revision with no copies (used to print a traceback):

  $ hg tip -v --template '\n'
  

Compact style works:

  $ hg log --style compact
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
  (available styles: bisect, changelog, compact, default, phases, xml)
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

Error on syntax:

  $ echo 'x = "f' >> t
  $ hg log
  abort: t:3: unmatched quotes
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

One common tag: longuest path wins:

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
