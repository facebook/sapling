#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ eagerepo
  $ setconfig remotenames.rename.default=
  $ setconfig remotenames.hoist=default

Pull master bookmark

  $ newclientrepo localrepo test:remoterepo
  $ cd ../remoterepo
  $ echo a > a
  $ hg add a
  $ hg commit -m 'First'
  $ hg book master

  $ cd ../localrepo
  $ hg pull
  pulling from test:remoterepo
  $ hg bookmarks --list-subscriptions
     default/master            1449e7934ec1

Create another bookmark on the remote repo
  $ cd ../remoterepo
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark master)
  $ hg book secondbook
  $ echo b >> a
  $ hg add a
  $ hg commit -m 'Commit secondbook points to'

Do not pull new boookmark from local repo
  $ cd ../localrepo
  $ hg pull
  pulling from test:remoterepo
  $ hg bookmarks --list-subscriptions
     default/master            1449e7934ec1

Do not pull new bookmark even if it on the same commit as old bookmark
  $ cd ../remoterepo
  $ hg up -q master
  $ hg book thirdbook
  $ hg book book-with-dashes
  $ cd ../localrepo
  $ hg pull
  pulling from test:remoterepo
  $ hg bookmarks --list-subscriptions
     default/master            1449e7934ec1

Move master bookmark
  $ cd ../remoterepo
  $ hg up -q master
  $ echo a >> a
  $ hg commit -m 'Move master bookmark'
  $ cd ../localrepo
  $ hg pull
  pulling from test:remoterepo
  imported commit graph for 1 commit (1 segment)
  $ hg bookmarks --list-subscriptions
     default/master            0238718db2b1

Specify bookmark to pull
  $ hg pull -B secondbook
  pulling from test:remoterepo
  searching for changes
  $ hg bookmarks --list-subscriptions
     default/master            0238718db2b1
     default/secondbook        ed7a9fd254d1

Create second remote
  $ cd ..
  $ cp -R remoterepo secondremoterepo
  $ cd secondremoterepo
  $ hg up -q 0238718db2b1
  $ hg book master --force
  $ cd ..

Add second remote repo path in localrepo
  $ cd localrepo
  $ setglobalconfig paths.secondremote=test:secondremoterepo
  $ hg pull secondremote
  pulling from test:secondremoterepo
  $ hg book --list-subscriptions
     default/master            0238718db2b1
     default/secondbook        ed7a9fd254d1
     secondremote/master       0238718db2b1

Move bookmark in second remote, pull and make sure it doesn't move in local repo
  $ cd ../secondremoterepo
  $ hg book secondbook --force
  $ echo aaa >> a
  $ hg commit -m 'Move bookmark in second remote'
  $ cd ../localrepo
  $ hg pull secondremote
  pulling from test:secondremoterepo

Move bookmark in first remote, pull and make sure it moves in local repo
  $ cd ../remoterepo
  $ hg up -q secondbook
  $ echo bbb > a
  $ hg commit -m 'Moves second bookmark'
  $ cd ../localrepo
  $ hg pull
  pulling from test:remoterepo
  searching for changes
  $ hg bookmarks --list-subscriptions
     default/master            0238718db2b1
     default/secondbook        c47dca9795c9
     secondremote/master       0238718db2b1

Delete bookmark on the server
  $ cd ../remoterepo
  $ hg book -d secondbook
  $ cd ../localrepo
  $ hg pull
  pulling from test:remoterepo
  $ hg bookmarks --list-subscriptions
     default/master            0238718db2b1
     secondremote/master       0238718db2b1

Update to the remote bookmark
  $ hg goto thirdbook --config 'remotenames.autopullhoistpattern=re:.*'
  pulling 'thirdbook' from 'test:remoterepo'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg book --verbose
  no bookmarks set
  $ hg book --list-subscriptions
     default/master            0238718db2b1
     default/thirdbook         1449e7934ec1
     secondremote/master       0238718db2b1

Trying to update to unknown bookmark
  $ hg goto unknownbook --config 'remotenames.autopullhoistpattern=re:.*'
  pulling 'unknownbook' from 'test:remoterepo'
  abort: unknown revision 'unknownbook'!
  [255]

  $ cd ..
  $ newclientrepo localrepo2 test:remoterepo
  $ hg pull
  pulling from test:remoterepo
  $ hg book --list-subscriptions
     default/master            0238718db2b1

By using "default/" the commit gets automatically pulled
  $ hg log -r default/thirdbook
  pulling 'thirdbook' from 'test:remoterepo'
  commit:      1449e7934ec1
  bookmark:    default/thirdbook
  hoistedname: thirdbook
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     First
  
Set two bookmarks in selectivepulldefault, make sure both of them were pulled
  $ setglobalconfig remotenames.selectivepulldefault=master,thirdbook

  $ hg pull -q
  $ hg book --list-subscriptions
     default/master            0238718db2b1
     default/thirdbook         1449e7934ec1

Check that `--remote` shows real remote bookmarks from default remote

  $ hg book --remote
     default/master                    0238718db2b174d2622ae9c4c75d61745eb12b25
     default/thirdbook                 1449e7934ec1c4d0c2eefb1194c1cb70e78ba232

  $ hg book --remote -Tjson
  [
   {
    "node": "0238718db2b174d2622ae9c4c75d61745eb12b25",
    "remotebookmark": "default/master"
   },
   {
    "node": "1449e7934ec1c4d0c2eefb1194c1cb70e78ba232",
    "remotebookmark": "default/thirdbook"
   }
  ]
  $ hg --config extensions.infinitepush= book --remote --remote-path test:secondremoterepo
     secondremote/master                    0238718db2b174d2622ae9c4c75d61745eb12b25
     secondremote/thirdbook                 1449e7934ec1c4d0c2eefb1194c1cb70e78ba232

when selectivepull is disabled

  $ hg book --remote --config remotenames.selectivepull=false
     default/master            0238718db2b1
     default/thirdbook         1449e7934ec1

  $ hg book --remote --config remotenames.selectivepull=false -Tjson
  [
   {
    "node": "0238718db2b174d2622ae9c4c75d61745eb12b25",
    "remotebookmark": "default/master"
   },
   {
    "node": "1449e7934ec1c4d0c2eefb1194c1cb70e78ba232",
    "remotebookmark": "default/thirdbook"
   }
  ]

  $ setglobalconfig remotenames.selectivepulldefault=master

Clone remote repo with the selectivepull enabled
  $ cd ..

  $ newclientrepo new_localrepo test:remoterepo

  $ hg book --list-subscriptions
     default/master            0238718db2b1

Check remote bookmarks after push
  $ hg up master -q
  $ echo "new commit to push" >> pushsh
  $ hg commit -qAm "push commit"
  $ hg push -r . --to master -q
  $ hg book --list-subscriptions
     default/master            a81520e7283a

Check the repo.pull API

  $ newrepo
  $ setconfig paths.default=test:remoterepo

- Pull nothing != Pull everything
  $ hg debugpull
  $ hg log -Gr 'all()' -T '{node} {desc} {remotenames}'

- Pull a name, and an unknown name. The unknown name is not an error (it will delete the name).
  $ hg debugpull -B thirdbook -B not-found
  $ hg log -Gr 'all()' -T '{node} {desc} {remotenames}'
  o  1449e7934ec1c4d0c2eefb1194c1cb70e78ba232 First default/thirdbook
  
- Only pull 'master' without 'thirdbook'. This will not work if remotenames expull is not bypassed.
  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ hg debugpull -B master
  $ hg log -Gr 'all()' -T '{node} {desc} {remotenames}'
  o  a81520e7283a6967ec1d82620b75ab92f5478638 push commit default/master
  │
  o  0238718db2b174d2622ae9c4c75d61745eb12b25 Move master bookmark
  │
  o  1449e7934ec1c4d0c2eefb1194c1cb70e78ba232 First
  
- Pull by hash + name + prefix
  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ hg debugpull -B thirdbook -r 0238718db2b174d2622ae9c4c75d61745eb12b25 -r 1449e7934ec1c
  $ hg log -Gr 'all()' -T '{node} {desc} {remotenames}'
  o  0238718db2b174d2622ae9c4c75d61745eb12b25 Move master bookmark
  │
  o  1449e7934ec1c4d0c2eefb1194c1cb70e78ba232 First default/thirdbook
  
- Auto pull in revset resolution

-- For remote bookmark names:

  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ setconfig remotenames.autopullpattern=
  $ hg log -r default/thirdbook::default/master -T '{node} {desc} {remotenames}\n'
  abort: unknown revision 'default/thirdbook'!
  [255]

  $ setconfig 'remotenames.autopullpattern=re:^default/[a-z]+$'
  $ hg log -r default/thirdbook::default/master -T '{node} {desc} {remotenames}\n'
  pulling 'master', 'thirdbook' from 'test:remoterepo'
  1449e7934ec1c4d0c2eefb1194c1cb70e78ba232 First default/thirdbook
  0238718db2b174d2622ae9c4c75d61745eb12b25 Move master bookmark 
  a81520e7283a6967ec1d82620b75ab92f5478638 push commit default/master

-- For hoisted remote bookmark names:

  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ setconfig remotenames.autopullhoistpattern=
  $ hg log -r thirdbook::master -T '{node} {desc} {remotenames}\n'
  abort: unknown revision 'thirdbook'!
  [255]
  $ setconfig 'remotenames.autopullhoistpattern=re:.*'
  $ hg log -r thirdbook::master -T '{node} {desc} {remotenames}\n'
  pulling 'thirdbook::master' from 'test:remoterepo'
  pulling 'master', 'thirdbook' from 'test:remoterepo'
  1449e7934ec1c4d0c2eefb1194c1cb70e78ba232 First default/thirdbook
  0238718db2b174d2622ae9c4c75d61745eb12b25 Move master bookmark 
  a81520e7283a6967ec1d82620b75ab92f5478638 push commit default/master

-- Names with revset operators can also be auto pulled.

  $ hg log -r book-with-dashes -T '{desc}\n'
  pulling 'book-with-dashes' from 'test:remoterepo'
  First

-- Quoting works:

  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ hg log -r '::"default/book-with-dashes"' -T '{desc}\n'
  pulling 'book-with-dashes' from 'test:remoterepo'
  First

-- For commit hashes:

  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ hg log -r '::1449e7934ec1c4d0c2eefb1194c1cb70e78ba232'
  pulling '1449e7934ec1c4d0c2eefb1194c1cb70e78ba232' from 'test:remoterepo'
  commit:      1449e7934ec1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     First
  

--- For "+" in revset expressions:

  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ hg log -r '023871+1449e793' -T "{node}\n"
  pulling '023871', '1449e793' from 'test:remoterepo'
  0238718db2b174d2622ae9c4c75d61745eb12b25
  1449e7934ec1c4d0c2eefb1194c1cb70e78ba232

  $ cd ../new_localrepo
  $ hg bundle -q -r a81520e7283a --base 0238718db2b1 ../bundle.hg

--- x will not be auto-pulled inside "present(x)":

  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ hg log -r 'present(023871)' -T "{node}\n"

--- x~y will not auto-pull `y`.

  $ newrepo
  $ setconfig paths.default=test:remoterepo
  $ hg log -r '.~1000000' -T "{node}\n"

--- autopull base revisions during unbundle

  $ newclientrepo unbundle test:empty
  $ setconfig paths.default=test:remoterepo
  $ hg unbundle ../bundle.hg
  pulling missing base commits: 0238718db2b174d2622ae9c4c75d61745eb12b25
  pulling from test:remoterepo
  adding changesets
  adding manifests
  adding file changes
