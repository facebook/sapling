# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ enable remotenames

Setup repo, and create test repo

  $ SCUBA_LOGGING_PATH="$TESTTMP/scuba.json"
  $ BLOB_TYPE="blob_files" EMIT_OBSMARKERS=1 quiet default_setup --with-dynamic-observability=true --scuba-log-file "$SCUBA_LOGGING_PATH"

  $ hg up -q "min(all())"

  $ mkdir -p root/a
  $ echo 'a/a' > root/a/a
  $ hg commit -Aqm "commit a"

  $ mkdir -p root/b
  $ echo 'b/b' > root/b/b
  $ hg commit -Aqm "commit b"

  $ mkdir -p root/c root/c/cc root/c/cc/ccc
  $ echo 'c/c' > root/c/c
  $ echo 'c/cc/c' > root/c/cc/c
  $ echo 'c/cc/ccc/c' > root/c/cc/ccc/c
  $ hg commit -Aqm "commit c"

Create the commit we'll look at later. Note the "," in the path name to test
escaping.

  $ mkdir -p root/d, root/d,/d root/d,/dd
  $ echo 'd/d/d' > root/d,/d/d
  $ echo 'd/dd/d' > root/d,/dd/d
  $ hg commit -Aqm "commit d"

  $ mkdir -p root/e
  $ echo 'e/e' > root/e/e
  $ echo 'd/dd/e' > root/d,/dd/e # Invalidate one of the trees from d
  $ hg commit -Aqm "commit e"

  $ mkdir -p root/f
  $ echo 'f/f' > root/f/f
  $ rm -r root/c # Remove all of c so it's not present locally
  $ hg commit -Aqm "commit f"

  $ hgmn push -q -r . --to master_bookmark

Setup a client repo that doesn't have any of the manifests in its local store.

  $ hgclone_treemanifest ssh://user@dummy/repo-hg test_repo --noupdate --config extensions.remotenames= -q
  $ cd test_repo
  $ hgmn pull -q -B master_bookmark

Setup some arguments to see debug output from remotefilelog

  $ LOG_ARGS=(--config ui.interactive=True --config remotefilelog.debug=True)

Fetch without designated nodes

  $ hgmn "${LOG_ARGS[@]}" --config "remotefilelog.cachepath=$TESTTMP/cache1" --config treemanifest.ondemandfetch=False show "master_bookmark~2" >/dev/null
  fetching tree for ('', 65b4f32575a18414983d65bbb6cdef3370aa582b)
  fetching tree '' 65b4f32575a18414983d65bbb6cdef3370aa582b
  1 trees fetched over 0.00s
  fetching tree for ('', 1595f1646547518ea8bb6f15db03fcaed5f98ab0)
  fetching tree '' 1595f1646547518ea8bb6f15db03fcaed5f98ab0
  1 trees fetched over 0.00s
  fetching 2 trees
  2 trees fetched over 0.00s
  fetching tree for ('root/d,', 0bc6688f4a1b0dca0ef82474e5fc62048eed3c2c)
  fetching tree 'root/d,' 0bc6688f4a1b0dca0ef82474e5fc62048eed3c2c
  1 trees fetched over 0.00s
  fetching 2 trees
  2 trees fetched over 0.00s

Fetch with designated ndoes

  $ hgmn "${LOG_ARGS[@]}" --config "remotefilelog.cachepath=$TESTTMP/cache2" --config treemanifest.ondemandfetch=True show "master_bookmark~2" >/dev/null
  fetching tree for ('', 65b4f32575a18414983d65bbb6cdef3370aa582b)
  fetching tree '' 65b4f32575a18414983d65bbb6cdef3370aa582b
  1 trees fetched over * (glob)
  fetching tree for ('', 1595f1646547518ea8bb6f15db03fcaed5f98ab0)
  fetching tree '' 1595f1646547518ea8bb6f15db03fcaed5f98ab0
  1 trees fetched over * (glob)
  fetching 2 trees
  2 trees fetched over * (glob)
  fetching tree for ('root/d,', 0bc6688f4a1b0dca0ef82474e5fc62048eed3c2c)
  fetching tree 'root/d,' 0bc6688f4a1b0dca0ef82474e5fc62048eed3c2c
  1 trees fetched over * (glob)
  fetching 2 trees
  2 trees fetched over * (glob)

Confirm that Mononoke logged commands, but didn't log any missing filenodes
  $ grep "Command processed" "$SCUBA_LOGGING_PATH" | wc -l
  36
  $ grep NullLinknode "$SCUBA_LOGGING_PATH"
  [1]

Confirm that we logged some reporting identifying designated nodes fetching too
  $ grep "Command processed" "$SCUBA_LOGGING_PATH" | jq .int.GettreepackDesignatedNodes | grep -Eqv '^0'
  $ grep "Gettreepack Params" "$SCUBA_LOGGING_PATH" | jq .int.gettreepack_directories | grep -Eqv '^0'

And check that the proper linknode was returned. Run this using hg, as opposed to hgmn, so we crash if it's not there.
  $ hg debugshell \
  > --config "remotefilelog.cachepath=$TESTTMP/cache2" \
  > -c 'ui.write("%s\n" % e.node.hex(mf.historystore.getnodeinfo("root/d,", e.node.bin("0bc6688f4a1b0dca0ef82474e5fc62048eed3c2c"))[2]))'
  e1972bf883fd70e2bb3bb68fe027099329ee854d

  $ hg log -r e1972bf883fd70e2bb3bb68fe027099329ee854d -T '{desc}\n'
  commit d
