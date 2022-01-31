# Takeover

The takeover directory holds the logic for the Takeover Client (the new EdenFS
process) and Server (the old EdenFS process) which are used during a graceful
restart process.

Takeover is currently supported for NFS and FUSE mounts. Takeover does not
support PrjFS mounts.

## Structure

There are 5 main components in the takeover directory: thrift serialization
library, client, server, data, and handler.


### Thrift serialization library

There are three main message classes that are exchanged over the takeover socket:

* `struct TakeoverVersionQuery` - This is sent from the client to the server to
inform the server what features of the takeover protocol the client supports.
This allows us to evolve the protocol. This struct contains two fields:
`versions` and `capabilities`.
    * `versions` is a legacy field. We use to use to distinguish versions of the
    takeover protocol by number. This list of versions is suppose to contain all
    the numbered versions of the protocol that the client supports. These days
    we expect the versions list to be a singleton list containing the last
    version number (7).
    * `capabilities` These days we have migrated the protocol to be capability
    based instead of version based. Instead of telling the server which versions
    the client supports, the client tells the server directly which features or
    capabilities of the takeover protocol the client supports. These
    capabilities are represented as a bit mask. Each bit represents a certain
    capability. You can see the different capabilities in `TakeoverData.h`. Some
    capabilities are required these days since we have deprecated old versions
    of the protocol and some capabilities have dependencies. See
    `TakeoverData::computeCompatibleCapabilities` and the comments in
    `TakeoverData.h`.
* empty "ready" ping - An empty ping sent by the server to ensure the client is
still alive and ready to receive takeover data
* `union SerializedTakeoverData` - This is a legacy union. Modern versions of
the protocol instead use `SerializedTakeoverResult` instead. This is a union of
a list of `SerializedMountInfo` or a string error.
    * `struct SerializedMountInfo` - see info in the lower section.
* `union SerializedTakeoverResult`. This is either a `SerializedTakeoverInfo`
or a string error. We migrated from `SerializedTakeoverData` to this so that
we can pass general (non mount point specific) data in the non error case.
    * `struct SerializedTakeoverInfo` - This is the struct representing all the
    data we send for takeover in the non error case. This contains two fields:
    `mounts` and `fileDescriptors`. `mounts` is a list of `SerializedMountInfo`
    and `fileDescriptors` is a list of `FileDescriptorType`.
        * `struct SerializedMountInfo` - Contains the mount path, the type
        of mount in use, a state directory, a list of bind mount paths
        (which is no longer used), connection information, and
        a `SerializedInodeMap`
            * `struct SerializedInodeMap` - A list of `SerializedInodeMapEntry`
            unloaded inodes
                * `struct SerializedInodeMapEntry` - contains inode
                information like inodeNumber, parentInode, name, isUnlinked,
                numFuseReferences, hash, and mode.
            * `struct SerializedFileHandleMap` - currently empty
    * `union FileDescriptorType` this is an enum of all the types of file
    descriptors we can send during takeover (excluding mount specific fds). The
    list of these represents the order the non mount specific file descriptors
    are sent in the underlying sendmsg call. Mount point file descriptors are
    sent in the same order as the list of SerializedMountInfo for the mount
    points.



### Client

The client has one function - `takeoverMounts`. This function requests to take
over mount points from an existing edenfs process. On success, it returns a
`TakeoverData` object, and it throws an exception on error. It takes four
parameters: a socketPath, a bool shouldPing, and a set of integers of supported
takeover versions (legacy), and a bit mask of the supported takeover versions.
The last three parameters are for testing purposes and should not be used in
productions builds.

This has a takeover timeout of 5 minutes for receiving takeover data from old
process.

We connect to the socket at the given path, then send our send our protocol
capabilities so that the server knows whether we're capable of handshaking
successfully. We then wait for the server to send us a "ready" ping, making sure
we are still listening on the socket. We respond to this ping and then wait for
the takeover data response. It is possible that we will not receive this ping,
and instead just receive the takeover data response.

After we get the takeover data response, we either throw an exception if we do
not get a message, or we deserialize the message and check its contents. We
throw an exception if the number of sockets sent is not the expected size
(num of mount points + 2 for the lock file and the thrift socket + 1 optional
socket for mountd). Otherwise, if all is well, we save the lock file,
thrift socket, mountd socket and all the mount points.


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
    server support a compatible takeover protocol capabilities.  If the
    capabilities are compatible, then the server starts to initiate shutdown
    by calling `server_->getTakeoverHandler()->startTakeoverShutdown()`.
    After the shutdown is completed, the takeover server pings the takeover
    client to ensure it is still waiting for the data. If the ping is
    unsuccessful (timeout, error, etc), the takeover server stops the takeover
    process and returns the untransmitted `TakeoverData` in an exception in
    order to let the `EdenServer` recover itself and start serving again.
    Finally, it closes its storage (local and backing stores) and sends the
    takeover data over the takeover socket by serializing the information
    (version, lock file, thrift socket, mount file descriptor) or error,
    and sending it.
* private functions:
    * `connectionAccepted` - callback function for allocating a connection
    handler when the server gets a client.
    * `acceptError` - callback function that simply logs on an accept() error on
    the takeover socket
    * `connectionDone` - callback function that is declared in the .h file but
    currently is not defined.

### Data

This holds the set of capabilities supported by this build. It also holds the
lock file, the server socket, mountd, expected order to serialize those file
descriptors, the mount points, and a takeover complete promise that
will be fulfilled by the `TakeoverServer` code once the `TakeoverData` has been
sent to the remote process. It has a function to serialize and deserialize
the `TakeoverData`.


### Handler

TakeoverHandler is a pure virtual interface for classes that want to implement
graceful takeover functionality. This is primarily implemented by the
`EdenServer` class.  However, there are also alternative implementations used
for unit testing.

It has two pure virtual functions: `startTakeoverShutdown()` and `closeStorage()`.

`startTakeoverShutdown()` will be called when a graceful shutdown has been
requested, with a remote process attempting to take over the currently running
mount points.

When implemented, this should return a Future that will produce the
`TakeoverData` to send to the remote edenfs process once the edenfs process is
ready to transfer its mounts.

`closeStorage()` will be called before sending the `TakeoverData` to the client,
conditionally on a successful ready handshake (if applicable).  This function
should close storage used by the server. In the case of an `EdenServer`, this
function allows for locks to be released in order for the new process to
take over this storage.
