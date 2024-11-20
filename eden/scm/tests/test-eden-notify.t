
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

# test notify changes-since
#
#   $ touch new_file
#   $ POSITION=$(eden notify get-position | get_prev_position) # becuase seq numberes differ based on platform
#   $ eden notification changes-since -p $POSITION | sort
#   * (glob)
#   position: *:*:0000000000000000000000000000000000000000 (glob)
#   small: added (Regular): 'new_file'
#   small: removed (Regular): '.hg/wlock' (windows !)
#   $ eden notification changes-since -p $POSITION --json
#   {"to_position":{"mount_generation":*,"sequence_number":*,"snapshot_hash":[*]},"changes":[{"SmallChange":{"Added":{"file_type":8,"path":[*]}}}]} (glob)
