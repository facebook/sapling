#require serve

Create a repo with whitespace only changes

  $ hg init repo-with-whitespace
  $ cd repo-with-whitespace
  $ cat > foo << EOF
  > line 0
  > line 1
  > line 2
  > line 3
  > EOF
  $ hg -q commit -A -m 'commit 0'
  $ cat > foo << EOF
  > line 0
  > line 1 modified by 1
  > line 2
  > line 3
  > EOF
  $ hg commit -m 'commit 1'
  $ cat > foo << EOF
  > line 0
  > line 1 modified by 1
  >     line 2
  > line 3
  > EOF
  $ hg commit -m 'commit 2 (leading whitespace on line 2)'
  $ cat > foo << EOF
  > line 0
  > line 1 modified by 1
  >     line 2
  > EOF
Need to use printf to avoid check-code complaining about trailing whitespace.
  $ printf 'line 3    \n' >> foo
  $ hg commit -m 'commit 3 (trailing whitespace on line 3)'
  $ cat > foo << EOF
  > line  0
  > line 1 modified by 1
  >     line 2
  > EOF
  $ printf 'line 3    \n' >> foo
  $ hg commit -m 'commit 4 (intra whitespace on line 0)'
  $ cat > foo << EOF
  > line  0
  > 
  > line 1 modified by 1
  >     line 2
  > EOF
  $ printf 'line 3    \n' >> foo
  $ hg commit -m 'commit 5 (add blank line between line 0 and 1)'
  $ cat > foo << EOF
  > line  0
  > 
  > 
  > line 1 modified by 1
  >     line 2
  > EOF
  $ printf 'line 3    \n' >> foo
  $ hg commit -m 'commit 6 (add another blank line between line 0 and 1)'

  $ hg log -G -T '{rev}:{node|short} {desc}'
  @  6:9d1b2c7db017 commit 6 (add another blank line between line 0 and 1)
  |
  o  5:400ef1d40470 commit 5 (add blank line between line 0 and 1)
  |
  o  4:08adbe269f24 commit 4 (intra whitespace on line 0)
  |
  o  3:dcb62cfbfc9b commit 3 (trailing whitespace on line 3)
  |
  o  2:6bdb694e7b8c commit 2 (leading whitespace on line 2)
  |
  o  1:23e1e37387dc commit 1
  |
  o  0:b9c578134d72 commit 0
  

  $ hg serve -p $HGPORT -d --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg serve --config annotate.ignorews=true -p $HGPORT1 -d --pid-file hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..

Annotate works

  $ get-with-headers.py --json $LOCALIP:$HGPORT 'json-annotate/9d1b2c7db017/foo'
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 4 (intra whitespace on line 0)",
        "line": "line  0\n",
        "lineno": 1,
        "node": "08adbe269f24cf22d975eadeec16790c5b22f558",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 5 (add blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 2,
        "node": "400ef1d404706cfb48afd2b78ce6addf641ced25",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 6 (add another blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 3,
        "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 1",
        "line": "line 1 modified by 1\n",
        "lineno": 4,
        "node": "23e1e37387dcfca4c0ed0cc568d1e4b9bfed241a",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 2 (leading whitespace on line 2)",
        "line": "    line 2\n",
        "lineno": 5,
        "node": "6bdb694e7b8cebb68d5b6b27b4bcc2a49d62c602",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 3 (trailing whitespace on line 3)",
        "line": "line 3    \n",
        "lineno": 6,
        "node": "dcb62cfbfc9b3ab995a5cbbaff6e1971c3e4f865",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 4
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "commit 6 (add another blank line between line 0 and 1)",
    "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
    "parents": [
      "400ef1d404706cfb48afd2b78ce6addf641ced25"
    ],
    "permissions": ""
  }

annotate.ignorews=1 config option is honored

  $ get-with-headers.py --json $LOCALIP:$HGPORT1 'json-annotate/9d1b2c7db017/foo'
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "line  0\n",
        "lineno": 1,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 5 (add blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 2,
        "node": "400ef1d404706cfb48afd2b78ce6addf641ced25",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 6 (add another blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 3,
        "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 1",
        "line": "line 1 modified by 1\n",
        "lineno": 4,
        "node": "23e1e37387dcfca4c0ed0cc568d1e4b9bfed241a",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "    line 2\n",
        "lineno": 5,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "line 3    \n",
        "lineno": 6,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 4
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "commit 6 (add another blank line between line 0 and 1)",
    "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
    "parents": [
      "400ef1d404706cfb48afd2b78ce6addf641ced25"
    ],
    "permissions": ""
  }

ignorews=1 query string argument enables whitespace skipping

  $ get-with-headers.py --json $LOCALIP:$HGPORT 'json-annotate/9d1b2c7db017/foo?ignorews=1'
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "line  0\n",
        "lineno": 1,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 5 (add blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 2,
        "node": "400ef1d404706cfb48afd2b78ce6addf641ced25",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 6 (add another blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 3,
        "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 1",
        "line": "line 1 modified by 1\n",
        "lineno": 4,
        "node": "23e1e37387dcfca4c0ed0cc568d1e4b9bfed241a",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "    line 2\n",
        "lineno": 5,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "line 3    \n",
        "lineno": 6,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 4
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "commit 6 (add another blank line between line 0 and 1)",
    "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
    "parents": [
      "400ef1d404706cfb48afd2b78ce6addf641ced25"
    ],
    "permissions": ""
  }

ignorews=0 query string argument disables when config defaults to enabled

  $ get-with-headers.py --json $LOCALIP:$HGPORT1 'json-annotate/9d1b2c7db017/foo?ignorews=0'
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 4 (intra whitespace on line 0)",
        "line": "line  0\n",
        "lineno": 1,
        "node": "08adbe269f24cf22d975eadeec16790c5b22f558",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 5 (add blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 2,
        "node": "400ef1d404706cfb48afd2b78ce6addf641ced25",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 6 (add another blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 3,
        "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 1",
        "line": "line 1 modified by 1\n",
        "lineno": 4,
        "node": "23e1e37387dcfca4c0ed0cc568d1e4b9bfed241a",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 2 (leading whitespace on line 2)",
        "line": "    line 2\n",
        "lineno": 5,
        "node": "6bdb694e7b8cebb68d5b6b27b4bcc2a49d62c602",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 3 (trailing whitespace on line 3)",
        "line": "line 3    \n",
        "lineno": 6,
        "node": "dcb62cfbfc9b3ab995a5cbbaff6e1971c3e4f865",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 4
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "commit 6 (add another blank line between line 0 and 1)",
    "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
    "parents": [
      "400ef1d404706cfb48afd2b78ce6addf641ced25"
    ],
    "permissions": ""
  }

ignorewsamount=1 query string enables whitespace amount skipping

  $ get-with-headers.py --json $LOCALIP:$HGPORT 'json-annotate/9d1b2c7db017/foo?ignorewsamount=1'
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "line  0\n",
        "lineno": 1,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 5 (add blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 2,
        "node": "400ef1d404706cfb48afd2b78ce6addf641ced25",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 6 (add another blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 3,
        "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 1",
        "line": "line 1 modified by 1\n",
        "lineno": 4,
        "node": "23e1e37387dcfca4c0ed0cc568d1e4b9bfed241a",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 2 (leading whitespace on line 2)",
        "line": "    line 2\n",
        "lineno": 5,
        "node": "6bdb694e7b8cebb68d5b6b27b4bcc2a49d62c602",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "line 3    \n",
        "lineno": 6,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 4
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "commit 6 (add another blank line between line 0 and 1)",
    "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
    "parents": [
      "400ef1d404706cfb48afd2b78ce6addf641ced25"
    ],
    "permissions": ""
  }

ignorewseol=1 query string enables whitespace end of line skipping

  $ get-with-headers.py --json $LOCALIP:$HGPORT 'json-annotate/9d1b2c7db017/foo?ignorewseol=1'
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 4 (intra whitespace on line 0)",
        "line": "line  0\n",
        "lineno": 1,
        "node": "08adbe269f24cf22d975eadeec16790c5b22f558",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 5 (add blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 2,
        "node": "400ef1d404706cfb48afd2b78ce6addf641ced25",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 6 (add another blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 3,
        "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 1",
        "line": "line 1 modified by 1\n",
        "lineno": 4,
        "node": "23e1e37387dcfca4c0ed0cc568d1e4b9bfed241a",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 2 (leading whitespace on line 2)",
        "line": "    line 2\n",
        "lineno": 5,
        "node": "6bdb694e7b8cebb68d5b6b27b4bcc2a49d62c602",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 0",
        "line": "line 3    \n",
        "lineno": 6,
        "node": "b9c578134d72b3a9d26afde8ddd76c0a93c5adbc",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 4
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "commit 6 (add another blank line between line 0 and 1)",
    "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
    "parents": [
      "400ef1d404706cfb48afd2b78ce6addf641ced25"
    ],
    "permissions": ""
  }

ignoreblanklines=1 query string enables whitespace blank line skipping

  $ get-with-headers.py --json $LOCALIP:$HGPORT 'json-annotate/9d1b2c7db017/foo?ignoreblanklines=1'
  200 Script output follows
  
  {
    "abspath": "foo",
    "annotate": [
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 4 (intra whitespace on line 0)",
        "line": "line  0\n",
        "lineno": 1,
        "node": "08adbe269f24cf22d975eadeec16790c5b22f558",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 1
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 5 (add blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 2,
        "node": "400ef1d404706cfb48afd2b78ce6addf641ced25",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 6 (add another blank line between line 0 and 1)",
        "line": "\n",
        "lineno": 3,
        "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 1",
        "line": "line 1 modified by 1\n",
        "lineno": 4,
        "node": "23e1e37387dcfca4c0ed0cc568d1e4b9bfed241a",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 2
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 2 (leading whitespace on line 2)",
        "line": "    line 2\n",
        "lineno": 5,
        "node": "6bdb694e7b8cebb68d5b6b27b4bcc2a49d62c602",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 3
      },
      {
        "abspath": "foo",
        "author": "test",
        "desc": "commit 3 (trailing whitespace on line 3)",
        "line": "line 3    \n",
        "lineno": 6,
        "node": "dcb62cfbfc9b3ab995a5cbbaff6e1971c3e4f865",
        "revdate": [
          0.0,
          0
        ],
        "targetline": 4
      }
    ],
    "author": "test",
    "children": [],
    "date": [
      0.0,
      0
    ],
    "desc": "commit 6 (add another blank line between line 0 and 1)",
    "node": "9d1b2c7db0175870a950f8c48c9c4ead1058f2c5",
    "parents": [
      "400ef1d404706cfb48afd2b78ce6addf641ced25"
    ],
    "permissions": ""
  }
