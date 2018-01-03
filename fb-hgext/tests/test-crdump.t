  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > drawdag=$RUNTESTDIR/drawdag.py
  > crdump=$TESTDIR/../hgext3rd/crdump.py
  > EOF

Create repo
  $ mkdir repo
  $ cd repo
  $ hg init
  $ echo A > a
  $ printf "A\0\n" > bin1
  $ hg addremove
  adding a
  adding bin1
  $ hg commit -m a
  $ hg phase -p .

  $ printf "A\nB\nC\nD\nE\nF\n" > a
  $ printf "a\0b\n" > bin1
  $ printf "b\0\n" > bin2
  $ hg addremove
  adding bin2
  $ hg commit -m "b
  > Differential Revision: https://phabricator.facebook.com/D123"

  $ echo G >> a
  $ echo C > c
  $ rm bin2
  $ echo x > bin1
  $ hg addremove
  removing bin2
  adding c
  $ hg commit -m c

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
                      "new_file": "5f54dc7f5b744f0bf88fcfe31eaba3cabc7a5f0c",
                      "old_file": "23c26c825bddcb198e701c6f7043a4e35dcb8b97"
                  },
                  {
                      "file_name": "bin2",
                      "new_file": "31f7b4d23cf93fd41972d0a879086e900cbf06c9",
                      "old_file": null
                  }
              ],
              "date": [
                  0,
                  0
              ],
              "desc": "b\nDifferential Revision: https://phabricator.facebook.com/D123",
              "files": [
                  "a",
                  "bin1",
                  "bin2"
              ],
              "node": "6370cd64643d547e11c6bc91920bca7b44ea21b5",
              "p1": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "patch_file": "6370cd64643d547e11c6bc91920bca7b44ea21b5.patch",
              "public_base": {
                  "node": "65d913976cc18347138f7b9f5186010d39b39b0f"
              },
              "user": "test"
          },
          {
              "binary_files": [
                  {
                      "file_name": "bin1",
                      "new_file": "4281f31b8cfa1376dc036a729c4118cd192db663",
                      "old_file": "5f54dc7f5b744f0bf88fcfe31eaba3cabc7a5f0c"
                  },
                  {
                      "file_name": "bin2",
                      "new_file": null,
                      "old_file": "31f7b4d23cf93fd41972d0a879086e900cbf06c9"
                  }
              ],
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
              "node": "c2c1919228a86d876dbb46befd0e0433c62a9f5f",
              "p1": {
                  "differential_revision": "123",
                  "node": "6370cd64643d547e11c6bc91920bca7b44ea21b5"
              },
              "patch_file": "c2c1919228a86d876dbb46befd0e0433c62a9f5f.patch",
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
  ...         print "#### commit %s" % commit['node']
  ...         print open(path.join(outdir, commit['patch_file'])).read()
  ...         for binfile in commit['binary_files']:
  ...             print "######## file %s" % binfile['file_name']
  ...             if binfile['old_file'] is not None:
  ...                 print "######## old"
  ...                 print open(path.join(outdir, binfile['old_file'])).read().encode('hex')
  ...             if binfile['new_file'] is not None:
  ...                 print "######## new"
  ...                 print open(path.join(outdir, binfile['new_file'])).read().encode('hex')
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
  #### commit 6370cd64643d547e11c6bc91920bca7b44ea21b5
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
  6100620a
  ######## file bin2
  ######## new
  62000a
  #### commit c2c1919228a86d876dbb46befd0e0433c62a9f5f
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
  6100620a
  ######## new
  780a
  ######## file bin2
  ######## old
  62000a



