#chg-compatible

  $ unset SCM_SAMPLING_FILEPATH
  $ LOGDIR=$TESTTMP/logs
  $ mkdir $LOGDIR

  $ disable treemanifest
  $ enable remotenames
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""

  $ enable sampling
  $ setconfig sampling.filepath=$LOGDIR/samplingpath.txt
  $ setconfig sampling.key.accessedremotenames=remotenames

  $ hg init remoterepo
  $ hg clone -q ssh://user@dummy/remoterepo localrepo

  $ mkcommit() {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "commit $1"
  > }

script to verify sampling
  $ cat > verifylast.py << EOF
  > import json, re
  > with open("$LOGDIR/samplingpath.txt") as f:
  >    data = f.read().strip("\0").split("\0")
  > if data:
  >     entry = json.loads(data[-1])
  >     if entry["category"] == "remotenames":
  >         if entry["data"]["metrics_type"] == "accessedremotenames":
  >             metrics = "accessedremotenames_totalnum"
  >             print("%s : %s" % (metrics, entry["data"][metrics]))
  > EOF

  $ checkaccessedbookmarks() {
  >    local file=.hg/selectivepullaccessedbookmarks
  >    if [ -f $file ]; then
  >        sort -k 3 $file ; python $TESTTMP/verifylast.py
  >    else
  >        echo "No contents!"
  >    fi
  > }


Setup remote repos

  $ cd remoterepo
  $ mkcommit BASE
  $ BASE=$(hg log -r . -T{node})

  $ mkcommit master
  $ hg book master

  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo secondremoterepo
  $ cd secondremoterepo
  $ hg up -q 206754acf7d8
  $ mkcommit new_master
  $ hg book master --force

  $ hg up '.^' -q
  $ mkcommit D
  $ hg book D_bookmark

  $ cd ../remoterepo

  $ hg up $BASE -q
  $ mkcommit A
  $ hg book A_bookmark

  $ hg up $BASE -q
  $ mkcommit B
  $ hg book B_bookmark

  $ hg up $BASE -q
  $ mkcommit C
  $ hg book C_bookmark

Check used remote bookmarks tracking

  $ cd ../localrepo
  $ setconfig remotenames.selectivepullaccessedbookmarks=True

  $ hg pull -q
  $ hg book --list-subscriptions
     default/A_bookmark        2:01c036b602a8
     default/B_bookmark        3:5b252c992f6d
     default/C_bookmark        4:d91e2f962bff
     default/master            1:206754acf7d8

  $ checkaccessedbookmarks
  No contents!
  $ hg pull -B master
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ checkaccessedbookmarks
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  accessedremotenames_totalnum : 1

  $ hg pull -B A_bookmark
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  accessedremotenames_totalnum : 2

Check pulling unknown bookmark

  $ hg pull -B unknown_book
  pulling from ssh://user@dummy/remoterepo
  abort: remote bookmark unknown_book not found!
  [255]
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  accessedremotenames_totalnum : 2

Add second remote to the paths and update to the first master

  $ cd ../localrepo
  $ cat >> $HGRCPATH << EOF
  > [paths]
  > secondremote=ssh://user@dummy/secondremoterepo
  > EOF
  $ hg pull secondremote -q
  $ hg book --list-subscriptions
     default/A_bookmark        2:01c036b602a8
     default/B_bookmark        3:5b252c992f6d
     default/C_bookmark        4:d91e2f962bff
     default/master            1:206754acf7d8
     secondremote/D_bookmark   6:a1b1b44a131d
     secondremote/master       5:a6b4ed81a38e

  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  accessedremotenames_totalnum : 2
  $ hg pull -B master secondremote
  pulling from ssh://user@dummy/secondremoterepo
  no changes found
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master
  accessedremotenames_totalnum : 3

Check pulling bookmark as a revset
TODO: need to log bookmarks passed under -r as well as normal

  $ hg pull -r C_bookmark
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master
  accessedremotenames_totalnum : 3

Check updating to the remote bookmark

  $ rm .hg/selectivepullaccessedbookmarks

  $ hg up default/A_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  accessedremotenames_totalnum : 1

  $ hg up secondremote/master
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master
  accessedremotenames_totalnum : 2

update to the hoisted name
  $ hg up B_bookmark
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  5b252c992f6da5179f90eda723431f54e5a9a3f5 bookmarks default/B_bookmark
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master
  accessedremotenames_totalnum : 3

change hoist and update again
  $ setconfig remotenames.hoist=secondremote

  $ hg up A_bookmark
  abort: unknown revision 'A_bookmark'!
  (if A_bookmark is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  5b252c992f6da5179f90eda723431f54e5a9a3f5 bookmarks default/B_bookmark
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master
  accessedremotenames_totalnum : 3

  $ hg up master
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  5b252c992f6da5179f90eda723431f54e5a9a3f5 bookmarks default/B_bookmark
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master
  accessedremotenames_totalnum : 3

Check selective pull setup using accessed bookmarks

  $ cd ../remoterepo
  $ mkcommit new_C
  $ hg log -G -T "'{desc}' {bookmarks} {remotenames}"
  @  'commit new_C' C_bookmark
  |
  o  'commit C'
  |
  | o  'commit B' B_bookmark
  |/
  | o  'commit A' A_bookmark
  |/
  | o  'commit master' master
  |/
  o  'commit BASE'
  

  $ cd ../secondremoterepo
  $ hg log -G -T "'{desc}' {bookmarks} {remotenames}"
  @  'commit D' D_bookmark
  |
  | o  'commit new_master' master
  |/
  o  'commit master'  default/master
  |
  o  'commit BASE'
  

  $ cd ../localrepo
  $ setconfig remotenames.hoist=default

  $ rm .hg/selectivepullaccessedbookmarks
  $ hg up C_bookmark
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ checkaccessedbookmarks
  d91e2f962bffb715a380162e945e6df13bf7aa3c bookmarks default/C_bookmark
  accessedremotenames_totalnum : 1
  $ hg book --list-subs
     default/A_bookmark        2:01c036b602a8
     default/B_bookmark        3:5b252c992f6d
     default/C_bookmark        4:d91e2f962bff
     default/master            1:206754acf7d8
     secondremote/D_bookmark   6:a1b1b44a131d
     secondremote/master       5:a6b4ed81a38e

  $ setconfig remotenames.selectivepull=True remotenames.selectivepulldefault=master
  $ hg pull
  pulling from ssh://user@dummy/remoterepo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg book --list-subs
     default/C_bookmark        7:d65ac0c5f8c9
     default/master            1:206754acf7d8
     secondremote/D_bookmark   6:a1b1b44a131d
     secondremote/master       5:a6b4ed81a38e
  $ sort .hg/selectivepullenabled
  default

pulling from the secondremote
  $ hg pull secondremote
  pulling from ssh://user@dummy/secondremoterepo
  no changes found
  $ hg book --list-subs
     default/C_bookmark        7:d65ac0c5f8c9
     default/master            1:206754acf7d8
     secondremote/master       5:a6b4ed81a38e
  $ sort .hg/selectivepullenabled
  default
  secondremote
