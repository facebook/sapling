Render records and conflict ranges compactly:

  $ cat > $TESTTMP/fmt.py << 'EOS'
  > import json, sys
  > d = json.load(sys.stdin)
  > for r in d["records"]:
  >     print("%s: %s merged=%r" % (r["id"], r["status"], r["merged"]))
  >     for c in r["conflicts"]:
  >         print("  conflict base=%r local=%r remote=%r merged[%d:%d]"
  >               % (c["base"], c["local"], c["remote"],
  >                  c["mergedStart"], c["mergedEnd"]))
  > EOS

The command needs no repository:

  $ cd $TESTTMP

Probe support without a merge request:

  $ sl debugmergetext --protocol-version
  {"protocolVersion": 1}

Clean merges preserve one-sided, identical, and disjoint edits:

  $ cat > clean.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "unchanged", "base": "a\nb\nc", "local": "a\nb\nc", "remote": "a\nb\nc"},
  >   {"id": "local-only", "base": "a\nb\nc", "local": "a\nLOCAL\nc", "remote": "a\nb\nc"},
  >   {"id": "remote-only", "base": "a\nb\nc", "local": "a\nb\nc", "remote": "a\nREMOTE\nc"},
  >   {"id": "identical", "base": "a\nb\nc", "local": "a\nBOTH\nc", "remote": "a\nBOTH\nc"},
  >   {"id": "disjoint", "base": "one\ntwo\nthree\nfour", "local": "one\nLOCAL\nthree\nfour", "remote": "one\ntwo\nthree\nREMOTE"}
  > ]}
  > EOS
  $ sl debugmergetext < clean.json | python $TESTTMP/fmt.py
  unchanged: clean merged='a\nb\nc'
  local-only: clean merged='a\nLOCAL\nc'
  remote-only: clean merged='a\nREMOTE\nc'
  identical: clean merged='a\nBOTH\nc'
  disjoint: clean merged='one\nLOCAL\nthree\nREMOTE'

Conflicts use local text, report splice ranges, and exit 0:

  $ cat > conflict.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "overlap", "base": "head\ncontested\ntail", "local": "head\nlocal side\ntail", "remote": "head\nremote side\ntail"}
  > ]}
  > EOS
  $ sl debugmergetext < conflict.json | python $TESTTMP/fmt.py
  overlap: conflict merged='head\nlocal side\ntail'
    conflict base='contested\n' local='local side\n' remote='remote side\n' merged[1:2]

  $ sl debugmergetext < conflict.json > /dev/null

Record ids and order are preserved, and a batch mixes outcomes freely:

  $ cat > batch.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "z-last", "base": "x", "local": "x", "remote": "x"},
  >   {"id": "a-first", "base": "p\nq\nr", "local": "P\nq\nr", "remote": "p\nq\nR"}
  > ]}
  > EOS
  $ sl debugmergetext < batch.json | python $TESTTMP/fmt.py
  z-last: clean merged='x'
  a-first: clean merged='P\nq\nR'

Preserve trailing newlines supplied by local or remote:

  $ cat > newline.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "none", "base": "a\nb\nc", "local": "a\nLOCAL\nc", "remote": "a\nb\nc"},
  >   {"id": "all", "base": "a\nb\nc\n", "local": "a\nLOCAL\nc\n", "remote": "a\nb\nc\n"},
  >   {"id": "local-only-lf", "base": "a\nb\nc", "local": "a\nLOCAL\nc\n", "remote": "a\nb\nc"},
  >   {"id": "empty", "base": "", "local": "", "remote": ""},
  >   {"id": "added-from-empty", "base": "", "local": "", "remote": "fresh"}
  > ]}
  > EOS
  $ sl debugmergetext < newline.json | python $TESTTMP/fmt.py
  none: clean merged='a\nLOCAL\nc'
  all: clean merged='a\nLOCAL\nc\n'
  local-only-lf: clean merged='a\nLOCAL\nc\n'
  empty: clean merged=''
  added-from-empty: clean merged='fresh'

Adjacent edits conflict; separated edits merge:

  $ cat > adjacent.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "touching", "base": "alpha\nbeta\ngamma", "local": "alpha\nBETA\ngamma", "remote": "alpha\nbeta\nGAMMA"},
  >   {"id": "separated", "base": "alpha\nbeta\nmiddle\ngamma\ndelta", "local": "alpha\nBETA\nmiddle\ngamma\ndelta", "remote": "alpha\nbeta\nmiddle\nGAMMA\ndelta"}
  > ]}
  > EOS
  $ sl debugmergetext < adjacent.json | python $TESTTMP/fmt.py
  touching: conflict merged='alpha\nBETA\ngamma'
    conflict base='beta\ngamma\n' local='BETA\ngamma\n' remote='beta\nGAMMA\n' merged[1:3]
  separated: clean merged='alpha\nBETA\nmiddle\nGAMMA\ndelta'

Unicode round-trips, including astral characters (compare UTF-8 hex):

  $ cat > unicode.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "unicode", "base": "café\nmiddle\nsecond", "local": "café\nmiddle\n🔥", "remote": "你好\nmiddle\nsecond"}
  > ]}
  > EOS
  $ sl debugmergetext < unicode.json | python -c 'import json,sys; r = json.load(sys.stdin)["records"][0]; print(r["status"], r["merged"].encode("utf-8").hex())'
  clean e4bda0e5a5bd0a6d6964646c650af09f94a5

Splice exact ranges across final lines, multiple conflicts, and deletions:

  $ cat > $TESTTMP/splice.py << 'EOS'
  > import json, sys
  > def resolve(r, side):
  >     lines = list(r["mergedLines"])
  >     for c in sorted(r["conflicts"], key=lambda c: -c["mergedStart"]):
  >         lines[c["mergedStart"]:c["mergedEnd"]] = [c[side]]
  >     out = "".join(lines)
  >     if r["finalNewlineStripped"] and out.endswith("\n"):
  >         out = out[:-1]
  >     return out
  > for r in json.load(sys.stdin)["records"]:
  >     print("%s: ranges=%r stripped=%s identity=%s remote=%r"
  >           % (r["id"], [(c["mergedStart"], c["mergedEnd"]) for c in r["conflicts"]],
  >              r["finalNewlineStripped"],
  >              resolve(r, "local") == r["merged"], resolve(r, "remote")))
  > EOS

  $ cat > splice.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "tail", "base": "a\nb", "local": "a\nX", "remote": "a\nY"},
  >   {"id": "tail-lf", "base": "a\nb\n", "local": "a\nX\n", "remote": "a\nY\n"},
  >   {"id": "multi", "base": "a\nC1\nb\nc\nd\nC2\ne", "local": "a\nL1\nb\nc\nd\nL2\ne", "remote": "a\nR1\nextra\nb\nc\nd\nR2\ne"},
  >   {"id": "deletion", "base": "a\nb\nc", "local": "a\nc", "remote": "a\nB\nc"},
  >   {"id": "empty-local", "base": "base", "local": "", "remote": "remote"},
  >   {"id": "empty-local-lf", "base": "base", "local": "", "remote": "remote\n"}
  > ]}
  > EOS
  $ sl debugmergetext < splice.json | python $TESTTMP/splice.py
  tail: ranges=[(1, 2)] stripped=True identity=True remote='a\nY'
  tail-lf: ranges=[(1, 2)] stripped=False identity=True remote='a\nY\n'
  multi: ranges=[(1, 2), (5, 6)] stripped=True identity=True remote='a\nR1\nextra\nb\nc\nd\nR2\ne'
  deletion: ranges=[(1, 1)] stripped=True identity=True remote='a\nB\nc'
  empty-local: ranges=[(0, 0)] stripped=True identity=True remote='remote'
  empty-local-lf: ranges=[(0, 0)] stripped=False identity=True remote='remote\n'

A trailing newline present only in base is removed:

  $ cat > terminator.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "base-only-lf", "base": "Test Plan: x\n", "local": "Test Plan: x", "remote": "Test Plan: x"},
  >   {"id": "base-only-lf-merged", "base": "a\nb\n", "local": "a\nLOCAL", "remote": "a\nb"}
  > ]}
  > EOS
  $ sl debugmergetext < terminator.json | python $TESTTMP/fmt.py
  base-only-lf: clean merged='Test Plan: x'
  base-only-lf-merged: clean merged='a\nLOCAL'

CRLF text round-trips verbatim:

  $ cat > crlf.json << 'EOS'
  > {"protocolVersion": 1, "records": [
  >   {"id": "crlf", "base": "a\r\nb\r\nc", "local": "a\r\nX\r\nc", "remote": "a\r\nb\r\nc"}
  > ]}
  > EOS
  $ sl debugmergetext < crlf.json | python $TESTTMP/fmt.py
  crlf: clean merged='a\r\nX\r\nc'

Protocol errors exit 1 without echoing input text:

  $ echo 'not json' | sl debugmergetext
  {"error": {"code": "INVALID_JSON"}, "protocolVersion": 1}
  [1]

  $ echo '[1,2,3]' | sl debugmergetext
  {"error": {"code": "INVALID_REQUEST"}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 99, "records": []}' | sl debugmergetext
  {"error": {"code": "UNSUPPORTED_PROTOCOL_VERSION"}, "protocolVersion": 1}
  [1]

Reject boolean and floating-point protocol versions:

  $ echo '{"protocolVersion": true, "records": []}' | sl debugmergetext
  {"error": {"code": "UNSUPPORTED_PROTOCOL_VERSION"}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 1.0, "records": []}' | sl debugmergetext
  {"error": {"code": "UNSUPPORTED_PROTOCOL_VERSION"}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 1, "records": "nope"}' | sl debugmergetext
  {"error": {"code": "INVALID_REQUEST"}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 1, "records": [{"id": "x", "base": "a", "local": "b"}]}' | sl debugmergetext
  {"error": {"code": "MISSING_FIELD", "recordId": "x", "recordIndex": 0, "side": "remote"}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 1, "records": [{"id": "x", "base": "a", "local": "b", "remote": 5}]}' | sl debugmergetext
  {"error": {"code": "INVALID_TYPE", "recordId": "x", "recordIndex": 0, "side": "remote"}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 1, "records": [{"base": "a", "local": "b", "remote": "c"}]}' | sl debugmergetext
  {"error": {"code": "MISSING_FIELD", "recordIndex": 0}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 1, "records": [{"id": "x", "base": "", "local": "", "remote": ""}, {"id": "x", "base": "", "local": "", "remote": ""}]}' | sl debugmergetext
  {"error": {"code": "DUPLICATE_RECORD_ID", "recordId": "x", "recordIndex": 1}, "protocolVersion": 1}
  [1]

NUL and lone surrogates are rejected rather than silently mangled:

  $ echo '{"protocolVersion": 1, "records": [{"id": "x", "base": "a\u0000b", "local": "b", "remote": "c"}]}' | sl debugmergetext
  {"error": {"code": "NUL_BYTE", "recordId": "x", "recordIndex": 0, "side": "base"}, "protocolVersion": 1}
  [1]

  $ echo '{"protocolVersion": 1, "records": [{"id": "x", "base": "\ud800", "local": "b", "remote": "c"}]}' | sl debugmergetext
  {"error": {"code": "LONE_SURROGATE", "recordId": "x", "recordIndex": 0, "side": "base"}, "protocolVersion": 1}
  [1]

  $ python -c 'import sys; sys.stdout.buffer.write(b"{\"protocolVersion\": 1, \"records\": [{\"id\": \"x\", \"base\": \"\xff\"}]}")' | sl debugmergetext
  {"error": {"code": "INVALID_UTF8"}, "protocolVersion": 1}
  [1]

An empty batch is valid and merges nothing:

  $ echo '{"protocolVersion": 1, "records": []}' | sl debugmergetext
  {"protocolVersion": 1, "records": []}

Enforce string, record, record-count, and request limits:

  $ python -c 'import json,sys; sys.stdout.write(json.dumps({"protocolVersion":1,"records":[{"id":"x","base":"a"*(2<<20),"local":"b","remote":"c"}]}))' | sl debugmergetext
  {"error": {"code": "STRING_TOO_LARGE", "recordId": "x", "recordIndex": 0, "side": "base"}, "protocolVersion": 1}
  [1]

  $ python -c 'import json,sys; n=1000000; sys.stdout.write(json.dumps({"protocolVersion":1,"records":[{"id":"x","base":"a"*n,"local":"b"*n,"remote":"c"*n}]}))' | sl debugmergetext
  {"error": {"code": "RECORD_TOO_LARGE", "recordId": "x", "recordIndex": 0}, "protocolVersion": 1}
  [1]

  $ python -c 'import json,sys; sys.stdout.write(json.dumps({"protocolVersion":1,"records":[{"id":str(i),"base":"","local":"","remote":""} for i in range(4097)]}))' | sl debugmergetext
  {"error": {"code": "TOO_MANY_RECORDS"}, "protocolVersion": 1}
  [1]

  $ python -c 'import json,sys; sys.stdout.write(json.dumps({"protocolVersion":1,"records":[{"id":"x","base":"a"*(5<<20),"local":"b","remote":"c"}]}))' > oversized.json
  $ sl debugmergetext < oversized.json
  {"error": {"code": "REQUEST_TOO_LARGE"}, "protocolVersion": 1}
  [1]
