
#require no-eden


  $ configure modern

Test the norepo endpoint (health):

  $ hg debugapi --config paths.default=test:e1
  {"server": "EagerRepo",
   "status": 200,
   "version": "HTTP/1.1",
   "request_id": None,
   "server_load": None,
   "tw_canary_id": None,
   "content_length": None,
   "tw_task_handle": None,
   "tw_task_version": None,
   "content_encoding": None}

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ hg push -r $B --to master --create -q

Test APIs:

  $ hg debugapi -e capabilities
  ["segmented-changelog",
   "commit-graph-segments",
   "sha1-only"]

  $ hg debugapi -e bookmarks -i '["master", "foo"]'
  {"foo": None,
   "master": "112478962961147124edd43549aedd1a335e44bf"}

  $ echo '["master", "foo"]' > names
  $ hg debugapi -e bookmarks -f names
  {"foo": None,
   "master": "112478962961147124edd43549aedd1a335e44bf"}

  $ hg debugapi -e commitdata -i "[b'$A']"
  [{"hgid": bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
    "revlog_data": b"\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\041b34f08c1356f6ad068e9ab9b43d984245111aa\ntest\n0 0\nA\n\nA"}]

  $ hg debugapi -e hashlookup -i '["11247", "33333"]'
  [{"hgids": [bin("112478962961147124edd43549aedd1a335e44bf")],
    "request": {"InclusiveRange": [bin("1124700000000000000000000000000000000000"),
                                   bin("11247fffffffffffffffffffffffffffffffffff")]}},
   {"hgids": [],
    "request": {"InclusiveRange": [bin("3333300000000000000000000000000000000000"),
                                   bin("33333fffffffffffffffffffffffffffffffffff")]}}]

  $ hg debugapi -e commitlocationtohash -i "[(b'$B',1,1)]"
  [{"count": 1,
    "hgids": [bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0")],
    "location": {"distance": 1,
                 "descendant": bin("112478962961147124edd43549aedd1a335e44bf")}}]

  $ hg debugapi -e commithashtolocation -i "[b'$B']" -i "[b'$A']"
  [{"hgid": bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
    "result": {"Ok": {"distance": 1,
                      "descendant": bin("112478962961147124edd43549aedd1a335e44bf")}}}]

  $ hg debugapi -e commitknown -i "[b'$B', b'$A', b'11111111111111111111']"
  [{"hgid": bin("112478962961147124edd43549aedd1a335e44bf"),
    "known": {"Ok": True}},
   {"hgid": bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
    "known": {"Ok": True}},
   {"hgid": bin("3131313131313131313131313131313131313131"),
    "known": {"Ok": False}}]

  $ hg debugapi -e commitknown -i "[b'$B', b'$A', b'11111111111111111111']" --sort
  [{"hgid": bin("3131313131313131313131313131313131313131"),
    "known": {"Ok": False}},
   {"hgid": bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
    "known": {"Ok": True}},
   {"hgid": bin("112478962961147124edd43549aedd1a335e44bf"),
    "known": {"Ok": True}}]

  $ hg debugapi -e clonedata
  {"idmap": {1: bin("112478962961147124edd43549aedd1a335e44bf")},
   "flat_segments": {"segments": [{"low": 0,
                                   "high": 1,
                                   "parents": []}]}}

  $ hg debugapi -e pullfastforwardmaster -i "b'$A'" -i "b'$B'"
  {"idmap": {0: bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
             1: bin("112478962961147124edd43549aedd1a335e44bf")},
   "flat_segments": {"segments": [{"low": 1,
                                   "high": 1,
                                   "parents": [0]}]}}

  $ hg debugapi -e pulllazy -i "[b'$A']" -i "[b'$B']"
  {"idmap": {0: bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0"),
             1: bin("112478962961147124edd43549aedd1a335e44bf")},
   "flat_segments": {"segments": [{"low": 1,
                                   "high": 1,
                                   "parents": [0]}]}}
  $ hg debugapi -e pulllazy -i "[]" -i "[b'$A']"
  {"idmap": {0: bin("426bada5c67598ca65036d57d9e4b64b0c1ce7a0")},
   "flat_segments": {"segments": [{"low": 0,
                                   "high": 0,
                                   "parents": []}]}}

  $ hg debugapi -e trees  -i '[("", "41b34f08c1356f6ad068e9ab9b43d984245111aa")]' -i '{"manifest_blob": True, "parents": True, "child_metadata": True, "augmented_trees":True }'
  [{"key": {"node": bin("41b34f08c1356f6ad068e9ab9b43d984245111aa"),
            "path": ""},
    "data": b"A\0005d992c5dcf32993668f7cede29d296c494a5d9\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("005d992c5dcf32993668f7cede29d296c494a5d9"),
                                          "path": "A"},
                                  "file_metadata": {"size": 1,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("6dcd4ce23d88e2ee9568ba546c007c63d9131c1b"),
                                                    "content_blake3": bin("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": {"augmented_manifest_id": bin("f0aef0c3978f2947b763a1f87ff5c68611125192cca9d0e95cb18787740eae3b"),
                      "augmented_manifest_size": 204}}]

  $ hg debugapi -e trees  -i '[("", "41b34f08c1356f6ad068e9ab9b43d984245111aa")]' -i '{"manifest_blob": True, "parents": True, "child_metadata": True}'
  [{"key": {"node": bin("41b34f08c1356f6ad068e9ab9b43d984245111aa"),
            "path": ""},
    "data": b"A\0005d992c5dcf32993668f7cede29d296c494a5d9\n",
    "parents": None,
    "children": [{"Ok": {"File": {"key": {"node": bin("005d992c5dcf32993668f7cede29d296c494a5d9"),
                                          "path": "A"},
                                  "file_metadata": {"size": 1,
                                                    "content_id": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "content_sha1": bin("6dcd4ce23d88e2ee9568ba546c007c63d9131c1b"),
                                                    "content_blake3": bin("5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda"),
                                                    "content_sha256": bin("0000000000000000000000000000000000000000000000000000000000000000"),
                                                    "file_header_metadata": b""}}}}],
    "tree_aux_data": None}]

Works outside repo
  $ hg --cwd .. debugapi test:e1 -e capabilities
  ["segmented-changelog",
   "commit-graph-segments",
   "sha1-only"]

