# Takeover

The takeover directory holds the logic for the Takeover Client (the new EdenFS
process) and Server (the old EdenFS process) whichare used during a graceful
restart process.

## Structure

There are 5 main components in the takeover directory: thrift serialization
library, client, server, data, and handler.


### Thrift serialization library

There are two main message classes:
* `struct TakeoverVersionQuery` - A list of takeover data serialization versions
that the client supports
* `union SerializedTakeoverData` - A list of `SerializedMountInfo` or a string
error.
** `struct SerializedMountInfo` - Contains the mount path, state directory, a
list of bind mount paths (which is no longer used), connection information, and
a `SerializedInodeMap`
** `struct SerializedInodeMapEntry` - contains inode information like
inodeNumber, parentInode, name, isUnlinked, numFuseReferences, hash, and mode.
** `struct SerializedInodeMap` - A list of `SerializedInodeMapEntry` unloaded
inodes
** `struct SerializedFileHandleMap` - currently empty

### Client

The client has one function - `takeoverMounts`. This function requests to take
over mount points from an existing edenfs process. On success, it returns a
`TakeoverData` object, and it throws an exception on error. It takes two
parameters: a socketPath, and a set of integers of supported takeover versions.

This has a takeover timeout of 5 minutes for receiving takeover data from old
process.

We connect to the socket at the given path, then send our send our protocol
version so that the server knows whether we're capable of handshaking
successfully. We then wait for the takeover data response.

After we get the takeover data response, we either throw an exception if we do
not get a message, or we deserialize the message and check its contents. We
throw an exception if the message is not the expected size
(num of mount points + 2 for the lock file and the thrift socket). Otherwise, if
all is well, we save the lock file, thrift socket, and all the mount points.


### Server

A helper class that listens on a unix domain socket for clients that wish to
perform graceful takeover of this `EdenServer`'s mount points. This class uses
the `EdenServer`'s main `EventBase` for driving its I/O.

It has a few functions:

* public function:
    * start - This is called when the EdenFS daemon first starts.  It begins
    listening on the takeover socket, waiting for a client to connect and
    request to initiate a graceful restart.  When a client connects, it verifies
    that the client process is from the same user ID, and that the client and
    server support a compatible takeover protocol version.  If the versions are
    compatible, then the server starts to initiate shutdown by calling return
    `server_->getTakeoverHandler()->startTakeoverShutdown()` Then, it sends the
    takeover data over the takeover socket by serializing the information
    (version, lock file, thrift socket, mount file descriptor) or error, and
    sending it.
* private functions:
    * `connectionAccepted` - callback function for allocating a connection
    handler when the server gets a client.
    * `acceptError` - callback function that simply logs on an accept() error on
    the takeover socket
    * `connectionDone` - callback function that is declared in the .h file but
    currently is not defined.

### Data

This holds the set of versions supported by this build. It also holds the lock
file, the server socket, the mount points, and a takeover complete promise that
will be fulfilled by the `TakeoverServer` code once the `TakeoverData` has been
sent to the remote process. It has a function to serialize and deserialize
the `TakeoverData`.


### Handler

TakeoverHandler is a pure virtual interface for classes that want to implement
graceful takeover functionality. This is primarily implemented by the
`EdenServer` class.  However, there are also alternative implementations used
for unit testing.

It has one pure virtual function called `startTakeoverShutdown()`.
startTakeoverShutdown() will be called when a graceful shutdown has been
requested, with a remote process attempting to take over the currently running
mount points.

When implemented, this should return a Future that will produce the
`TakeoverData` to send to the remote edenfs process once the edenfs process is
ready to transfer its mounts.
