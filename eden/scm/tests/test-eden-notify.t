
#require eden

setup backing repo

  $ newclientrepo

utils

    @command
    def get_prev_position(args, stdin, stdout, fs):
      jp = stdin.read().decode("utf-8").split(":")
      last_seq = int(jp[1])-1
      stdout.write(f"{jp[0]}:{last_seq}:{jp[2]}".encode())

test eden notify get-position

  $ eden notify get-position
  *:*:0000000000000000000000000000000000000000 (glob)
  $ eden notification get-position
  *:*:0000000000000000000000000000000000000000 (glob)
  $ eden notification get-position --json
  {"mount_generation":*,"sequence_number":*,"snapshot_hash":[*]} (glob)

test notify changes-since
  $ eden notification changes-since -p 0:0:0000000000000000000000000000000000000000
  large: lost_changes EdenFsRemounted
  position: *:*:0000000000000000000000000000000000000000 (glob)
  $ eden notification changes-since -p 0:0:0000000000000000000000000000000000000000 --json --formatted-position
  {"changes":[{"LargeChange":{"LostChanges":{"reason":"EdenFsRemounted"}}}],"to_position":"*:*:*"} (glob)
  $ touch new_file
  $ POSITION=$(eden notify get-position | get_prev_position) # becuase seq numberes differ based on platform
  $ eden notification changes-since -p $POSITION | sort
  * (glob)
  position: *:*:0000000000000000000000000000000000000000 (glob)
  small: added (Regular): 'new_file'
  $ eden notification changes-since -p $POSITION --json
  {"to_position":{"mount_generation":*,"sequence_number":*,"snapshot_hash":[*]},"changes":[{"SmallChange":{"Added":{"file_type":"Regular","path":[*]}}}]} (glob)
