  $ enable remotenames
  $ setconfig ui.ssh="python \"$TESTDIR/dummyssh\""
  $ hg init remoterepo
  $ hg clone -q ssh://user@dummy/remoterepo localrepo

  $ mkcommit() {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "commit $1"
  > }

  $ checkaccessedbookmarks() {
  >    local file=.hg/selectivepullaccessedbookmarks
  >    if [ -f $file ]; then
  >        sort -k 3 $file
  >    else
  >        echo "No contents!"
  >    fi
  > }

Create remote bookmarks

  $ cd remoterepo
  $ mkcommit BASE
  $ BASE=$(hg log -r . -T{node})

  $ mkcommit master
  $ hg book master

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
  $ hg bookmarks --remote
     default/A_bookmark        2:01c036b602a8
     default/B_bookmark        3:5b252c992f6d
     default/C_bookmark        4:d91e2f962bff
     default/master            1:206754acf7d8

  $ checkaccessedbookmarks
  $ hg pull -B master
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ checkaccessedbookmarks
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  $ hg pull -B A_bookmark
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master

Check pulling unknown bookmark

  $ hg pull -B unknown_book
  pulling from ssh://user@dummy/remoterepo
  abort: remote bookmark unknown_book not found!
  [255]
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master

Add second remote and update to first master

  $ cd ..
  $ hg clone -q ssh://user@dummy/remoterepo secondremoterepo
  $ cd secondremoterepo
  $ hg up -q 206754acf7d8
  $ mkcommit new_master

  $ hg book master --force

  $ cd ../localrepo
  $ cat >> $HGRCPATH << EOF
  > [paths]
  > secondremote=ssh://user@dummy/secondremoterepo
  > EOF
  $ hg pull secondremote -q
  $ hg book --remote
     default/A_bookmark        2:01c036b602a8
     default/B_bookmark        3:5b252c992f6d
     default/C_bookmark        4:d91e2f962bff
     default/master            1:206754acf7d8
     secondremote/master       5:a6b4ed81a38e

  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  $ hg pull -B master secondremote
  pulling from ssh://user@dummy/secondremoterepo
  no changes found
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master

Check pulling bookmark as a revset
TODO: need to log bookmarks passed under -r as well as normal

  $ hg pull -r C_bookmark
  pulling from ssh://user@dummy/remoterepo
  no changes found
  $ checkaccessedbookmarks
  01c036b602a86df67ef1a00e4b0266d23c8fafee bookmarks default/A_bookmark
  206754acf7d8d6a9d471f64406dc10c55a13db13 bookmarks default/master
  a6b4ed81a38e7d63d6b8ed66264a1fecd0ae90ef bookmarks secondremote/master
