#debugruntest-compatible
#require no-windows no-eden
  $ configure modernclient

  $ cat > make-request.py <<'EOF'
  > import sys, struct
  > cmd = "\0".join(sys.argv[1:]).encode()
  > sys.stdout.buffer.write(b"runcommand\n" + struct.pack(">I", len(cmd)) + cmd)
  > EOF

  $ cat > read-response.py <<'EOF'
  > import sys, struct
  > first = True
  > while channel_type := sys.stdin.buffer.read(1):
  >   msg_len = struct.unpack(">I", sys.stdin.buffer.read(4))[0]
  >   msg = sys.stdin.buffer.read(msg_len)
  >   if first:
  >     # skip "hello" message
  >     first = False
  >     continue
  >   if channel_type == b"r":
  >     msg = str(struct.unpack(">i", msg)[0]).encode("utf-8") + b"\n"
  >   if msg != b"\n":
  >     msg = b"from " + channel_type + b": " + msg
  >   sys.stdout.buffer.write(msg)
  > EOF

Works outside repo (and with Rust command):
  $ python ~/make-request.py debug-args some-arg | hg serve --cmdserver pipe | python ~/read-response.py
  from o: ["some-arg"]
  from r: 0

Works in repo:
  $ newclientrepo repo
  $ touch foo
  $ python ~/make-request.py status | hg serve --cmdserver pipe | python ~/read-response.py
  from o: ? foo
  from r: 0

Test an error result just for fun:
  $ python ~/make-request.py status --rev nope | hg serve --cmdserver pipe | python ~/read-response.py
  from e: abort: unknown revision 'nope'!
  from r: 255
