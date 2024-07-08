
#require eden fsmonitor no-windows

setup backing repo
  $ newclientrepo
  $ watchman watch $TESTTMP/repo1 > /dev/null
  $ mkdir base
  $ cd base
  $ mkdir depth1
  $ touch txt.txt
  $ ln -s txt.txt sym.link

Watchman commands using eden
# check composite functions can return directories
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "relative_root": "base",
  > "expression": ["not", "false"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "d",
              "name": "depth1"
          },
          {
              "type": "l",
              "name": "sym.link"
          },
          {
              "type": "f",
              "name": "txt.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "relative_root": "base",
  > "expression": ["allof", "true", "true"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "d",
              "name": "depth1"
          },
          {
              "type": "l",
              "name": "sym.link"
          },
          {
              "type": "f",
              "name": "txt.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "relative_root": "base",
  > "expression": ["anyof", "true", "false"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "d",
              "name": "depth1"
          },
          {
              "type": "l",
              "name": "sym.link"
          },
          {
              "type": "f",
              "name": "txt.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
