
#require eden fsmonitor no-windows

setup backing repo

  $ cat > $TESTTMP/.edenrc <<EOF
  > [glob]
  > use-edenapi-suffix-query = true
  > EOF
  $ newclientrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
#if no-windows
  $ eden restart 2>1 > /dev/null
#else
  $ eden --home-dir $TESTTMP restart 2>1 > /dev/null
#endif
  $ eden debug logging eden/fs/service=DBG4 > /dev/null
  $ watchman watch $TESTTMP/repo1 > /dev/null
  $ mkdir base
  $ cd base
  $ mkdir depth1.txt
  $ touch txt.txt
  $ ln -s txt.txt symlink.txt
  $ cd ..


watchman without files flag will only look at local
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "glob": ["**/*.txt"],
  > "expression": ["not", "false"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "d",
              "name": "base/depth1.txt"
          },
          {
              "type": "l",
              "name": "base/symlink.txt"
          },
          {
              "type": "f",
              "name": "base/txt.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count")'
    "store.sapling.fetch_glob_files_success.count": 0,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 1,


watchman with d file type expression will only look at local
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "glob": ["**/*.txt"],
  > "expression": ["type", "d"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "d",
              "name": "base/depth1.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count")'
    "store.sapling.fetch_glob_files_success.count": 0,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 2,


test missing files will trigger fallback
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "glob": ["**/*.txt"],
  > "expression": ["type", "f"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "f",
              "name": "base/txt.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
# this value represents the success of the EdenAPI call, not the success of the overall globFiles usage of it
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count")'
    "store.sapling.fetch_glob_files_success.count": 1,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 2,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count": 1,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count": 0,


watchman with not d file type expression will use edenAPI
  $ hg checkout $A > /dev/null
  $ touch foo.txt
  $ touch baz.txt
  $ hg add foo.txt
  $ hg add baz.txt
  $ hg amend 2> /dev/null
  $ hg checkout 4c6d6cef04fa > /dev/null
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "glob": ["**/*.txt"],
  > "expression": ["not", ["type", "d"]],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "l",
              "name": "base/symlink.txt"
          },
          {
              "type": "f",
              "name": "base/txt.txt"
          },
          {
              "type": "f",
              "name": "baz.txt"
          },
          {
              "type": "f",
              "name": "foo.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count")'
    "store.sapling.fetch_glob_files_success.count": 2,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 2,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count": 1,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count": 1,


watchman with f or l file type expression will use edenAPI
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "glob": ["**/*.txt"],
  > "expression": ["type", "f"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "f",
              "name": "base/txt.txt"
          },
          {
              "type": "f",
              "name": "baz.txt"
          },
          {
              "type": "f",
              "name": "foo.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count")'
    "store.sapling.fetch_glob_files_success.count": 3,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 2,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count": 1,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count": 2,
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "glob": ["**/*.txt"],
  > "expression": ["type", "l"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "l",
              "name": "base/symlink.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count")'
    "store.sapling.fetch_glob_files_success.count": 4,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 2,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count": 1,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count": 3,



watchman suffix generator uses edenAPI
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "suffix": ["txt"],
  > "expression": ["type", "f"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "f",
              "name": "base/txt.txt"
          },
          {
              "type": "f",
              "name": "baz.txt"
          },
          {
              "type": "f",
              "name": "foo.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count")'
    "store.sapling.fetch_glob_files_success.count": 5,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 2,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count": 1,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count": 4,


watchman suffix expression does not use edenAPI because it resolves to ** and post-filters the results
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "expression": ["allof", ["suffix", "txt"], ["type", "f"]],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "f",
              "name": ".hg/last-message.txt"
          },
          {
              "type": "f",
              "name": "base/txt.txt"
          },
          {
              "type": "f",
              "name": "baz.txt"
          },
          {
              "type": "f",
              "name": "foo.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
#if osx
  $ sleep 10
#endif
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count")'
    "store.sapling.fetch_glob_files_success.count": 5,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 3,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count": 1,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count": 4,

Deleting a file locally will hide it from display even if it's in the remote
  $ rm foo.txt
  $ watchman -j <<-EOT
  > ["query", "$TESTTMP/repo1", {
  > "suffix": ["txt"],
  > "expression": ["type", "f"],
  > "fields": ["name", "type"]
  > }]
  > EOT
  {
      * (glob)
      "files": [
          {
              "type": "f",
              "name": "base/txt.txt"
          },
          {
              "type": "f",
              "name": "baz.txt"
          }
      ],
      * (glob)
      "clock": * (glob)
      "debug": {
          "cookie_files": []
      }
  }
  $ sleep 10
  $ eden debug thrift getCounters --json | egrep '(store.sapling.fetch_glob_files_success.count"|thrift.EdenServiceHandler.glob_files.local_success.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count"|thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count")'
    "store.sapling.fetch_glob_files_success.count": 6,
    "thrift.EdenServiceHandler.glob_files.local_success.count": 3,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_fallback.count": 1,
    "thrift.EdenServiceHandler.glob_files.sapling_remote_api_success.count": 5,
