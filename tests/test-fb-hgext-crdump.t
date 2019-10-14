  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > crdump=
  > remotenames=
  > [experimental]
  > evolution.createmarkers=True
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > EOF
  $ showgraph() {
  >   hg log --graph --hidden -T "{rev} {desc|firstline}" | sed \$d
  > }

Create repo
  $ mkdir server
  $ hg init server
  $ hg clone -q ssh://user@dummy/server repo
  $ cd repo
  $ echo A > a
  $ printf "A\0\n" > bin1
  $ hg addremove
  adding a
  adding bin1
  $ hg commit -m a
  $ hg push -q -r . --to releasebranch --create
  $ hg phase -p .

  $ printf "A\nB\nC\nD\nE\nF\n" > a
  $ printf "a\0b\n" > bin1
  $ printf "b\0\n" > bin2
  $ hg addremove
  adding bin2
  $ revision="Differential Revision: https://phabricator.facebook.com/D123"
  $ hg commit -m "b
  > $revision"

  $ showgraph
  @  1 b
  |
  o  0 a

Test obsolete markers

  $ printf "a\0b\0c\n" > bin1
  $ hg amend -m "b'
  > $revision"
  $ showgraph
  @  2 b'
  |
  | x  1 b
  |/
  o  0 a
  $ hg debugcrdump -U 1 -r . --obsolete --traceback
  {
      "commits": [
          {
              "binary_files": [
                  {
                      "file_name": "bin1",
                      "new_file": "69403266b0b9ca9c9403df5097a07d01b74f3e23",
                      "old_file": "23c26c825bddcb198e701c6f7043a4e35dcb8b97"
                  },
                  {
                      "file_name": "bin2",
                      "new_file": "31f7b4d23cf93fd41972d0a879086e900cbf06c9",
                      "old_file": null
                  }
              ],
              "bookmarks": [],
              "branch": "releasebranch",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "b'\nDifferential Revision: https://phabricator.facebook.com/D123",
              "files": [
                  "a",
                  "bin1",
                  "bin2"
              ],
              "node": "9e6c8a14e241d3140575d17288d4a91bd8c9a3c8",
              "obsolete": [
                  {
                      "date": [
                          0.0,
                          0
                      ],
                      "flag": 0,
                      "metadata": {
                          "operation": "amend",
                          "user": "test"
                      },
                      "prednode": "6370cd64643d547e11c6bc91920bca7b44ea21b5",
                      "succnodes": [
                          "9e6c8a14e241d3140575d17288d4a91bd8c9a3c8"
                      ]
                  }
              ],
              "p1": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "patch_file": "9e6c8a14e241d3140575d17288d4a91bd8c9a3c8.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          }
      ],
      "output_directory": "*" (glob)
  }


  $ echo G >> a
  $ echo C > c
  $ rm bin2
  $ echo x > bin1
  $ hg addremove
  removing bin2
  adding c
  $ hg commit -m c
  $ hg bookmark bookmark1 -i

Add a master bookmark and verify it becomes the remote branch
- The [1] exit code is because no commits are pushed
  $ hg push -q -r releasebranch --to master --create
  [1]

Test basic dump of two commits

  $ hg debugcrdump -U 1 -r ".^^::." --traceback| tee ../json_output
  {
      "commits": [
          {
              "binary_files": [
                  {
                      "file_name": "bin1",
                      "new_file": "23c26c825bddcb198e701c6f7043a4e35dcb8b97",
                      "old_file": null
                  }
              ],
              "bookmarks": [],
              "branch": "master",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "a",
              "files": [
                  "a",
                  "bin1"
              ],
              "node": "65d913976cc18347138f7b9f5186010d39b39b0f",
              "p1": {
                  "node": "0000000000000000000000000000000000000000"
              },
              "patch_file": "65d913976cc18347138f7b9f5186010d39b39b0f.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          },
          {
              "binary_files": [
                  {
                      "file_name": "bin1",
                      "new_file": "69403266b0b9ca9c9403df5097a07d01b74f3e23",
                      "old_file": "23c26c825bddcb198e701c6f7043a4e35dcb8b97"
                  },
                  {
                      "file_name": "bin2",
                      "new_file": "31f7b4d23cf93fd41972d0a879086e900cbf06c9",
                      "old_file": null
                  }
              ],
              "bookmarks": [],
              "branch": "master",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "b'\nDifferential Revision: https://phabricator.facebook.com/D123",
              "files": [
                  "a",
                  "bin1",
                  "bin2"
              ],
              "node": "9e6c8a14e241d3140575d17288d4a91bd8c9a3c8",
              "p1": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "patch_file": "9e6c8a14e241d3140575d17288d4a91bd8c9a3c8.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          },
          {
              "binary_files": [
                  {
                      "file_name": "bin1",
                      "new_file": "08f31c375398e39fe9c485a2a06a79dfc296580e",
                      "old_file": "69403266b0b9ca9c9403df5097a07d01b74f3e23"
                  },
                  {
                      "file_name": "bin2",
                      "new_file": null,
                      "old_file": "31f7b4d23cf93fd41972d0a879086e900cbf06c9"
                  }
              ],
              "bookmarks": [
                  "bookmark1"
              ],
              "branch": "master",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "c",
              "files": [
                  "a",
                  "bin1",
                  "bin2",
                  "c"
              ],
              "node": "e3a67aeeade9ad9e292f1762f8f075a8322042b7",
              "p1": {
                  "differential_revision": "123",
                  "node": "9e6c8a14e241d3140575d17288d4a91bd8c9a3c8"
              },
              "patch_file": "e3a67aeeade9ad9e292f1762f8f075a8322042b7.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          }
      ],
      "output_directory": "*" (glob)
  }

  >>> import json
  >>> from os import path
  >>> with open("../json_output") as f:
  ...     data = json.loads(f.read())
  ...     outdir = data['output_directory']
  ...     for commit in data['commits']:
  ...         print("#### commit %s" % commit['node'])
  ...         print(open(path.join(outdir, commit['patch_file'])).read())
  ...         for binfile in commit['binary_files']:
  ...             print("######## file %s" % binfile['file_name'])
  ...             if binfile['old_file'] is not None:
  ...                 print("######## old")
  ...                 print(open(path.join(outdir, binfile['old_file'])).read().encode('hex'))
  ...             if binfile['new_file'] is not None:
  ...                 print("######## new")
  ...                 print(open(path.join(outdir, binfile['new_file'])).read().encode('hex'))
  ...     import shutil
  ...     shutil.rmtree(outdir)
  #### commit 65d913976cc18347138f7b9f5186010d39b39b0f
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,1 @@
  +A
  diff --git a/bin1 b/bin1
  new file mode 100644
  Binary file bin1 has changed
  
  ######## file bin1
  ######## new
  41000a
  #### commit 9e6c8a14e241d3140575d17288d4a91bd8c9a3c8
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,6 @@
   A
  +B
  +C
  +D
  +E
  +F
  diff --git a/bin1 b/bin1
  Binary file bin1 has changed
  diff --git a/bin2 b/bin2
  new file mode 100644
  Binary file bin2 has changed
  
  ######## file bin1
  ######## old
  41000a
  ######## new
  61006200630a
  ######## file bin2
  ######## new
  62000a
  #### commit e3a67aeeade9ad9e292f1762f8f075a8322042b7
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -6,1 +6,2 @@
   F
  +G
  diff --git a/bin1 b/bin1
  Binary file bin1 has changed
  diff --git a/bin2 b/bin2
  deleted file mode 100644
  Binary file bin2 has changed
  diff --git a/c b/c
  new file mode 100644
  --- /dev/null
  +++ b/c
  @@ -0,0 +1,1 @@
  +C
  
  ######## file bin1
  ######## old
  61006200630a
  ######## new
  780a
  ######## file bin2
  ######## old
  62000a




#if jq
Test crdump not dumping binaries

  $ hg debugcrdump -U 1 -r ".^^::." | jq '.commits[].binary_files?'
  [
    {
      "file_name": "bin1",
      "new_file": "23c26c825bddcb198e701c6f7043a4e35dcb8b97",
      "old_file": null
    }
  ]
  [
    {
      "file_name": "bin1",
      "new_file": "69403266b0b9ca9c9403df5097a07d01b74f3e23",
      "old_file": "23c26c825bddcb198e701c6f7043a4e35dcb8b97"
    },
    {
      "file_name": "bin2",
      "new_file": "31f7b4d23cf93fd41972d0a879086e900cbf06c9",
      "old_file": null
    }
  ]
  [
    {
      "file_name": "bin1",
      "new_file": "08f31c375398e39fe9c485a2a06a79dfc296580e",
      "old_file": "69403266b0b9ca9c9403df5097a07d01b74f3e23"
    },
    {
      "file_name": "bin2",
      "new_file": null,
      "old_file": "31f7b4d23cf93fd41972d0a879086e900cbf06c9"
    }
  ]

  $ hg debugcrdump -U 1 -r ".^^::." --nobinary | jq '.commits[].binary_files?'
  null
  null
  null
#endif

Test non-ASCII characters

  $ echo x > X
  $ HGENCODING=utf-8 hg commit -Aqm "Méssage în únicode"
  $ HGENCODING=utf-8 hg book -r . "unusúal-bøøkmàrk"
  $ hg debugcrdump -r .
  {
      "commits": [
          {
              "binary_files": [],
              "bookmarks": [
                  "unus\u00faal-b\u00f8\u00f8km\u00e0rk"
              ],
              "branch": "master",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "M\u00e9ssage \u00een \u00fanicode",
              "files": [
                  "X"
              ],
              "node": "4d5bdcf868416c46f75e4a118b69d8022325bcda",
              "p1": {
                  "differential_revision": "",
                  "node": "e3a67aeeade9ad9e292f1762f8f075a8322042b7"
              },
              "patch_file": "4d5bdcf868416c46f75e4a118b69d8022325bcda.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          }
      ],
      "output_directory": "*" (glob)
  }
  $ hg debugcrdump -r . --encoding=utf-8
  {
      "commits": [
          {
              "binary_files": [],
              "bookmarks": [
                  "unus\u00faal-b\u00f8\u00f8km\u00e0rk"
              ],
              "branch": "master",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "M\u00e9ssage \u00een \u00fanicode",
              "files": [
                  "X"
              ],
              "node": "4d5bdcf868416c46f75e4a118b69d8022325bcda",
              "p1": {
                  "differential_revision": "",
                  "node": "e3a67aeeade9ad9e292f1762f8f075a8322042b7"
              },
              "patch_file": "4d5bdcf868416c46f75e4a118b69d8022325bcda.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          }
      ],
      "output_directory": "*" (glob)
  }
  $ hg debugcrdump -r . --encoding=iso-8859-1
  {
      "commits": [
          {
              "binary_files": [],
              "bookmarks": [
                  "unus\u00faal-b\u00f8\u00f8km\u00e0rk"
              ],
              "branch": "master",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "M\u00e9ssage \u00een \u00fanicode",
              "files": [
                  "X"
              ],
              "node": "4d5bdcf868416c46f75e4a118b69d8022325bcda",
              "p1": {
                  "differential_revision": "",
                  "node": "e3a67aeeade9ad9e292f1762f8f075a8322042b7"
              },
              "patch_file": "4d5bdcf868416c46f75e4a118b69d8022325bcda.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          }
      ],
      "output_directory": "*" (glob)
  }
  $ hg debugcrdump -r . --encoding=ascii
  {
      "commits": [
          {
              "binary_files": [],
              "bookmarks": [
                  "unus\u00faal-b\u00f8\u00f8km\u00e0rk"
              ],
              "branch": "master",
              "commit_cloud": false,
              "date": [
                  0,
                  0
              ],
              "desc": "M\u00e9ssage \u00een \u00fanicode",
              "files": [
                  "X"
              ],
              "node": "4d5bdcf868416c46f75e4a118b69d8022325bcda",
              "p1": {
                  "differential_revision": "",
                  "node": "e3a67aeeade9ad9e292f1762f8f075a8322042b7"
              },
              "patch_file": "4d5bdcf868416c46f75e4a118b69d8022325bcda.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          }
      ],
      "output_directory": "*" (glob)
  }

#if jq
Test use globalrev instead of svnrev

  $ echo >> Y
  $ hg commit -Aqm "commit with no globalrev"
  $ hg phase -pfr '.'
  $ echo >> Z
  $ hg commit -Aqm "local commit"
  $ hg debugcrdump -r '.' | jq -e '.commits[].public_base.svnrev' > /dev/null
  [1]
  $ hg -q update '.^'
  $ echo >> Y
  $ hg commit --config "extensions.commitextras=" \
  > -Aqm "commit with globalrev" --extra global_rev="100098765"
  $ hg phase -pfr '.'
  $ echo >> Z
  $ hg commit -Aqm "local commit"
  $ hg debugcrdump -r '.' | jq -e '.commits[].public_base.svnrev' > /dev/null
  [1]
  $ hg debugcrdump --config "extensions.globalrevs=" -r '.' \
  > | jq '.commits[].public_base.svnrev'
  "100098765"
#endif
