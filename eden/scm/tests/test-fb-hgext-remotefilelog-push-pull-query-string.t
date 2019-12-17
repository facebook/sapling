#chg-compatible

  $ setconfig extensions.treemanifest=!
  $ . "$TESTDIR/library.sh"

  $ unset SCM_SAMPLING_FILEPATH
  $ LOGDIR=$TESTTMP/logs
  $ mkdir $LOGDIR
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > logginghelper=
  > remotenames=
  > sampling=
  > [sampling]
  > filepath = $LOGDIR/samplingpath.txt
  > key.logginghelper=logginghelper
  > EOF

  $ hg init repo
  $ cd repo
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ hg book master
  $ echo x >> x
  $ hg commit -qAm x2

Test that query parameters are ignored when grouping paths, so that
when pushing to one path, the bookmark for the other path gets updated
as well

  $ cd ..
  $ hgcloneshallow ssh://user@dummy/repo client -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s
  $ cd client
  $ hg path
  default = ssh://user@dummy/repo
  $ hg path -a default ssh://user@dummy/repo?read
  $ hg path -a default-push ssh://user@dummy/repo?write
  $ hg path
  default = ssh://user@dummy/repo?read
  default-push = ssh://user@dummy/repo?write
  $ hg log -r .
  changeset:   1:a89d614e2364
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x2
  
  $ echo x >> x
  $ hg commit -qAm x3
  $ hg push --to master
  pushing rev 421535db10b6 to destination ssh://user@dummy/repo?write bookmark master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark master
  $ hg log -r .
  changeset:   2:421535db10b6
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x3
  
  $ hg pull
  pulling from ssh://user@dummy/repo?read
  searching for changes
  no changes found
  $ hg log -r .
  changeset:   2:421535db10b6
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x3
  
Verify logging uses correct repo name
  $ cat > logverify.py << EOF
  > import json
  > with open("$LOGDIR/samplingpath.txt") as f:
  >    data = f.read().strip("\0").split("\0")
  > alldata = {}
  > for jsonstr in data:
  >     entry = json.loads(jsonstr)
  >     if entry["category"] == "logginghelper":
  >         for k in sorted(entry["data"].keys()):
  >             if k == "repo" and entry["data"][k] != None:
  >                 print("%s: %s" % (k, entry["data"][k]))
  > EOF
  $ python logverify.py | uniq
  repo: repo
