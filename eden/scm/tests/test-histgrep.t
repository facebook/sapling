#chg-compatible
#debugruntest-compatible

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init t
  $ cd t
  $ echo import > port
  $ hg add port
  $ hg commit -m 0 -u spam -d '0 0'
  $ echo export >> port
  $ hg commit -m 1 -u eggs -d '1 0'
  $ echo export > port
  $ echo vaportight >> port
  $ echo 'import/export' >> port
  $ hg commit -m 2 -u spam -d '2 0'
  $ echo 'import/export' >> port
  $ hg commit -m 3 -u eggs -d '3 0'
  $ head -n 3 port > port1
  $ mv port1 port
  $ hg commit -m 4 -u spam -d '4 0'

pattern error

  $ hg histgrep '**test**'
  grep: invalid match pattern: nothing to repeat* (glob)
  [1]

simple

  $ hg histgrep '.*'
  port:914fa752cdea:export
  port:914fa752cdea:vaportight
  port:914fa752cdea:import/export
  $ hg histgrep port port
  port:914fa752cdea:export
  port:914fa752cdea:vaportight
  port:914fa752cdea:import/export

simple with color

  $ hg --config extensions.color= histgrep --config color.mode=ansi \
  >     --color=always port port
  \x1b[35mport\x1b[39m\x1b[36m:\x1b[39m914fa752cdea\x1b[36m:\x1b[39mex\x1b[0m\x1b[1m\x1b[31mport\x1b[0m (esc)
  \x1b[35mport\x1b[39m\x1b[36m:\x1b[39m914fa752cdea\x1b[36m:\x1b[39mva\x1b[0m\x1b[1m\x1b[31mport\x1b[0might (esc)
  \x1b[35mport\x1b[39m\x1b[36m:\x1b[39m914fa752cdea\x1b[36m:\x1b[39mim\x1b[0m\x1b[1m\x1b[31mport\x1b[0m/ex\x1b[0m\x1b[1m\x1b[31mport\x1b[0m (esc)

simple templated

  $ hg histgrep port \
  > -T '{file}:{node|short}:{texts % "{if(matched, text|upper, text)}"}\n'
  port:914fa752cdea:exPORT
  port:914fa752cdea:vaPORTight
  port:914fa752cdea:imPORT/exPORT

simple JSON (no "change" field)

  $ hg histgrep -Tjson port
  [
   {
    "date": [4.0, 0],
    "file": "port",
    "line_number": 1,
    "node": "914fa752cdea87777ac1a8d5c858b0c736218f6c",
    "rev": 4,
    "texts": [{"matched": false, "text": "ex"}, {"matched": true, "text": "port"}],
    "user": "spam"
   },
   {
    "date": [4.0, 0],
    "file": "port",
    "line_number": 2,
    "node": "914fa752cdea87777ac1a8d5c858b0c736218f6c",
    "rev": 4,
    "texts": [{"matched": false, "text": "va"}, {"matched": true, "text": "port"}, {"matched": false, "text": "ight"}],
    "user": "spam"
   },
   {
    "date": [4.0, 0],
    "file": "port",
    "line_number": 3,
    "node": "914fa752cdea87777ac1a8d5c858b0c736218f6c",
    "rev": 4,
    "texts": [{"matched": false, "text": "im"}, {"matched": true, "text": "port"}, {"matched": false, "text": "/ex"}, {"matched": true, "text": "port"}],
    "user": "spam"
   }
  ]

simple JSON without matching lines

  $ hg histgrep -Tjson -l port
  [
   {
    "date": [4.0, 0],
    "file": "port",
    "line_number": 1,
    "node": "914fa752cdea87777ac1a8d5c858b0c736218f6c",
    "rev": 4,
    "user": "spam"
   }
  ]

all

  $ hg histgrep --traceback --all -nu port port
  port:914fa752cdea:4:-:spam:import/export
  port:95040cfd017d:4:+:eggs:import/export
  port:3b325e3481a1:1:-:spam:import
  port:3b325e3481a1:2:-:spam:export
  port:3b325e3481a1:1:+:spam:export
  port:3b325e3481a1:2:+:spam:vaportight
  port:3b325e3481a1:3:+:spam:import/export
  port:8b20f75c1585:2:+:eggs:export
  port:f31323c92170:1:+:spam:import

all JSON

  $ hg histgrep --all -Tjson port port
  [
   {
    "change": "-",
    "date": [4.0, 0],
    "file": "port",
    "line_number": 4,
    "node": "914fa752cdea87777ac1a8d5c858b0c736218f6c",
    "rev": 4,
    "texts": [{"matched": false, "text": "im"}, {"matched": true, "text": "port"}, {"matched": false, "text": "/ex"}, {"matched": true, "text": "port"}],
    "user": "spam"
   },
   {
    "change": "+",
    "date": [3.0, 0],
    "file": "port",
    "line_number": 4,
    "node": "95040cfd017d658c536071c6290230a613c4c2a6",
    "rev": 3,
    "texts": [{"matched": false, "text": "im"}, {"matched": true, "text": "port"}, {"matched": false, "text": "/ex"}, {"matched": true, "text": "port"}],
    "user": "eggs"
   },
   {
    "change": "-",
    "date": [2.0, 0],
    "file": "port",
    "line_number": 1,
    "node": "3b325e3481a1f07435d81dfdbfa434d9a0245b47",
    "rev": 2,
    "texts": [{"matched": false, "text": "im"}, {"matched": true, "text": "port"}],
    "user": "spam"
   },
   {
    "change": "-",
    "date": [2.0, 0],
    "file": "port",
    "line_number": 2,
    "node": "3b325e3481a1f07435d81dfdbfa434d9a0245b47",
    "rev": 2,
    "texts": [{"matched": false, "text": "ex"}, {"matched": true, "text": "port"}],
    "user": "spam"
   },
   {
    "change": "+",
    "date": [2.0, 0],
    "file": "port",
    "line_number": 1,
    "node": "3b325e3481a1f07435d81dfdbfa434d9a0245b47",
    "rev": 2,
    "texts": [{"matched": false, "text": "ex"}, {"matched": true, "text": "port"}],
    "user": "spam"
   },
   {
    "change": "+",
    "date": [2.0, 0],
    "file": "port",
    "line_number": 2,
    "node": "3b325e3481a1f07435d81dfdbfa434d9a0245b47",
    "rev": 2,
    "texts": [{"matched": false, "text": "va"}, {"matched": true, "text": "port"}, {"matched": false, "text": "ight"}],
    "user": "spam"
   },
   {
    "change": "+",
    "date": [2.0, 0],
    "file": "port",
    "line_number": 3,
    "node": "3b325e3481a1f07435d81dfdbfa434d9a0245b47",
    "rev": 2,
    "texts": [{"matched": false, "text": "im"}, {"matched": true, "text": "port"}, {"matched": false, "text": "/ex"}, {"matched": true, "text": "port"}],
    "user": "spam"
   },
   {
    "change": "+",
    "date": [1.0, 0],
    "file": "port",
    "line_number": 2,
    "node": "8b20f75c158513ff5ac80bd0e5219bfb6f0eb587",
    "rev": 1,
    "texts": [{"matched": false, "text": "ex"}, {"matched": true, "text": "port"}],
    "user": "eggs"
   },
   {
    "change": "+",
    "date": [0.0, 0],
    "file": "port",
    "line_number": 1,
    "node": "f31323c9217050ba245ee8b537c713ec2e8ab226",
    "rev": 0,
    "texts": [{"matched": false, "text": "im"}, {"matched": true, "text": "port"}],
    "user": "spam"
   }
  ]

other

  $ hg histgrep -l port port
  port:914fa752cdea
  $ hg histgrep import port
  port:914fa752cdea:import/export

  $ hg cp port port2
  $ hg commit -m 4 -u spam -d '5 0'

follow

  $ hg histgrep --traceback -f 'import\n\Z' port2
  port:f31323c92170:import
  
  $ echo deport >> port2
  $ hg commit -m 5 -u eggs -d '6 0'
  $ hg histgrep -f --all -nu port port2
  port2:1a78f9325f49:4:+:eggs:deport
  port:914fa752cdea:4:-:spam:import/export
  port:95040cfd017d:4:+:eggs:import/export
  port:3b325e3481a1:1:-:spam:import
  port:3b325e3481a1:2:-:spam:export
  port:3b325e3481a1:1:+:spam:export
  port:3b325e3481a1:2:+:spam:vaportight
  port:3b325e3481a1:3:+:spam:import/export
  port:8b20f75c1585:2:+:eggs:export
  port:f31323c92170:1:+:spam:import

  $ hg up -q null
  $ hg histgrep -f port
  [1]

  $ cd ..
  $ hg init t2
  $ cd t2
  $ hg histgrep foobar foo
  [1]
  $ hg histgrep foobar
  [1]
  $ echo blue >> color
  $ echo black >> color
  $ hg add color
  $ hg ci -m 0
  $ echo orange >> color
  $ hg ci -m 1
  $ echo black > color
  $ hg ci -m 2
  $ echo orange >> color
  $ echo blue >> color
  $ hg ci -m 3
  $ hg histgrep orange
  color:e0116d3829f8:orange
  $ hg histgrep --all orange
  color:e0116d3829f8:+:orange
  color:11bd8bc8d653:-:orange
  color:7c585a21e0d1:+:orange

test substring match: '^' should only match at the beginning

  $ hg histgrep '^.' --config extensions.color= --color debug
  [grep.filename|color][grep.sep|:][grep.node|e0116d3829f8][grep.sep|:][grep.match|b]lack
  [grep.filename|color][grep.sep|:][grep.node|e0116d3829f8][grep.sep|:][grep.match|o]range
  [grep.filename|color][grep.sep|:][grep.node|e0116d3829f8][grep.sep|:][grep.match|b]lue

match in last "line" without newline

  $ printf "no infinite loop" > noeol
  $ hg ci -Amnoeol
  adding noeol
  $ hg histgrep loop
  noeol:8c97710679c8:no infinite loop

  $ cd ..

Issue685: traceback in grep -r after rename

Got a traceback when using grep on a single
revision with renamed files.

  $ hg init issue685
  $ cd issue685
  $ echo octarine > color
  $ hg ci -Amcolor
  adding color
  $ hg rename color colour
  $ hg ci -Am rename
  $ hg histgrep octarine
  colour:efd8f9e6d7a7:octarine
  color:ec548afd7026:octarine

Used to crash here

  $ hg histgrep -r 'desc(rename)' octarine
  colour:efd8f9e6d7a7:octarine
  $ cd ..


Issue337: test that grep follows parent-child relationships instead
of just using revision numbers.

  $ hg init issue337
  $ cd issue337

  $ echo white > color
  $ hg commit -A -m "0 white"
  adding color

  $ echo red > color
  $ hg commit -A -m "1 red"

  $ hg goto 'desc(0)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo black > color
  $ hg commit -A -m "2 black"

  $ hg goto --clean 'desc(1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo blue > color
  $ hg commit -A -m "3 blue"

  $ hg histgrep --all red
  color:787a36e93381:-:red
  color:3e2bd43f9d34:+:red

  $ cd ..

  $ hg init a
  $ cd a
  $ cp "$TESTDIR/binfile.bin" .
  $ hg add binfile.bin
  $ hg ci -m 'add binfile.bin'
  $ hg histgrep "MaCam" --all
  binfile.bin:48b371597640:+: Binary file matches

  $ cd ..
