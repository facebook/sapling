# Takeover

The takeover directory holds the logic for the Takeover Client (the new EdenFS
process) and Server (the old EdenFS process) which are used during a graceful
restart process.

Takeover is currently supported for NFS and FUSE mounts. Takeover does not
support PrjFS mounts.

## Overview

The takeover process allows a new EdenFS daemon to seamlessly take over mount
points from an existing running daemon without unmounting the filesystems. This
enables graceful restarts where the user's experience is minimally disrupted.

The key components involved are:

- **TakeoverClient**: The new EdenFS process that requests to take over mounts
- **TakeoverServer**: The old EdenFS process that hands off its mounts
- **TakeoverData**: The data structure containing all information needed for
  takeover
- **TakeoverHandler**: Interface implemented by EdenServer for takeover
  operations

## Structure

There are 5 main components in the takeover directory: thrift serialization
library, client, server, data, and handler.

### Thrift Serialization Library (`takeover.thrift`)

The thrift file defines the message types exchanged over the takeover socket:

#### Version and Capability Negotiation

- `struct TakeoverVersionQuery` - Sent from the client to the server to inform
  the server what features of the takeover protocol the client supports. This
  struct contains two fields:
  - `versions` - A legacy field containing a set of supported protocol version
    numbers. Modern clients send the singleton set containing version 7. After
    version 7, we use capabilities for new features. This field can be removed
    in favor of capabilities.
  - `capabilities` - A 64-bit bitmask indicating which features the client
    supports. This is the preferred method for protocol negotiation as it allows
    for more granular feature matching.

#### Message Types

- **Empty "ready" ping** - In version 4, a ping message introduced that sent by
  the server to ensure the client is still alive and ready to receive takeover
  data before actually sending it. This prevents the server from attempting to
  transfer mounts to a disconnected client.

- **Chunked message markers** - For large takeover data (e.g., 3+ million
  inodes), the data is split into chunks:
  - `FIRST_CHUNK` - Signals the start of chunked data transfer
  - `LAST_CHUNK` - Signals the end of chunked data transfer
  - Chunk size defined in `FLAGS_maximumChunkSize` is default to 512 MB

#### Data Structures

- `union SerializedTakeoverResult` - The modern format for takeover data. This
  is either:
  - `SerializedTakeoverInfo` - Contains the takeover data on success
  - `string errorReason` - Contains an error message on failure

- `struct SerializedTakeoverInfo` - Contains:
  - `mounts` - A list of `SerializedMountInfo` for each mount point
  - `fileDescriptors` - A list of `FileDescriptorType` indicating which file
    descriptors are being transferred and in what order

- `struct SerializedMountInfo` - Contains mount-specific data:
  - `mountPath` - The path where the filesystem is mounted
  - `stateDirectory` - The directory containing EdenFS state for this mount
  - `bindMountPaths` - Legacy field, no longer used
  - `connInfo` - For FUSE mounts, a binary blob containing the `fuse_init_out`
    structure (left empty for NFS mounts)
  - `inodeMap` - A `SerializedInodeMap` containing unloaded inode information
  - `mountProtocol` - The type of mount (FUSE, NFS, or UNKNOWN)

- `struct SerializedInodeMap` - Contains:
  - `unloadedInodes` - A list of `SerializedInodeMapEntry`

- `struct SerializedInodeMapEntry` - Contains inode metadata:
  - `inodeNumber` - The inode number
  - `parentInode` - The parent inode number
  - `name` - The entry name
  - `isUnlinked` - Whether the inode has been unlinked
  - `numFsReferences` - Number of filesystem references
  - `hash` - Optional object hash (unset means materialized)
  - `mode` - The inode mode bits

- `enum FileDescriptorType` - Types of file descriptors transferred:
  - `LOCK_FILE` - The EdenFS lock file
  - `THRIFT_SOCKET` - The thrift server socket
  - `MOUNTD_SOCKET` - The NFS mountd socket (optional, only for NFS mounts)

- `enum TakeoverMountProtocol` - Mount protocol types:
  - `UNKNOWN` - Unknown/unspecified (legacy)
  - `FUSE` - FUSE mount
  - `NFS` - NFS mount

- `union SerializedTakeoverData` - **Deprecated.** Legacy format used by older
  versions. Modern versions use `SerializedTakeoverResult` instead.

### Client (`TakeoverClient.cpp`)

The client provides the `takeoverMounts` function which requests to take over
mount points from an existing edenfs process. On success, it returns a
`TakeoverData` object; on error, it throws an exception.

**Parameters:**

- `socketPath` - Path to the takeover unix socket
- `takeoverReceiveTimeout` - Timeout for receiving takeover data
- `shouldThrowDuringTakeover` - For testing: simulate an error during takeover
- `shouldPing` - For testing: whether to respond to the ready ping
- `supportedVersions` - Set of supported protocol versions
- `supportedTakeoverCapabilities` - Bitmask of supported capabilities

**Protocol Flow:**

1. Connect to the takeover socket at the specified path with a 1-second timeout
2. Send a `TakeoverVersionQuery` containing supported versions and capabilities
3. Wait for the server's response, which can be:
   - A **ping message** - The client responds with an empty message to confirm
     it's still alive, then waits for the actual takeover data
   - A **FIRST_CHUNK marker** - Indicates the server is sending chunked data;
     the client recursively receives chunks until receiving LAST_CHUNK
   - **Takeover data directly** - Older servers (eden versions older than
     June 2025) may send data without ping/chunking
4. Deserialize the received message into `TakeoverData`
5. Validate the received data (correct number of file descriptors, etc.)
6. Extract the lock file, thrift socket, mountd socket (if present), and mount
   point file descriptors

### Server (`TakeoverServer.cpp`)

A helper class that listens on a unix domain socket for clients that wish to
perform graceful takeover of this `EdenServer`'s mount points. Uses the
`EdenServer`'s main `EventBase` for driving I/O.

**Public Interface:**

- `TakeoverServer(eventBase, socketPath, handler, faultInjector, supportedVersions, supportedCapabilities)` -
  Constructor that initializes and starts the server
- `start()` - Begins listening on the takeover socket

**Internal Connection Handling (`ConnHandler`):**

When a client connects, the server:

1. **Validates credentials** - Checks that the connecting process has the same
   UID as the server process (security check)

2. **Receives version query** - Waits up to 5 seconds for the client to send its
   supported versions and capabilities

3. **Negotiates protocol** - Computes the compatible version and capabilities
   between client and server. Capabilities are computed as the intersection of
   what both sides support.

4. **Initiates shutdown** - Calls `handler->startTakeoverShutdown()` to begin
   the graceful shutdown process. This returns a Future that completes with
   `TakeoverData` when the server is ready to transfer.

5. **Pings the client** (if PING capability is supported) - Sends a ping message
   to verify the client is still connected. Waits up to 5 seconds (configurable
   via `pingReceiveTimeout` flag) for a response. If the ping fails, the server
   recovers and resumes normal operation.

6. **Closes storage** - Calls `handler->closeStorage()` to release locks on
   local and backing stores so the new process can acquire them.

7. **Sends takeover data** - Serializes and sends the `TakeoverData`. For large
   datasets, uses chunked transfer:
   - Sends FIRST_CHUNK marker
   - Sends data chunks (first chunk includes file descriptors)
   - Sends LAST_CHUNK marker

8. **Signals completion** - Fulfills the `takeoverComplete` promise to notify
   the EdenServer that takeover is finished.

**Error Handling:**

- If any step fails, the server sends an error message to the client
- If the ping fails, the server returns the `TakeoverData` through the promise
  so EdenServer can recover and resume serving

### Data (`TakeoverData.h` / `TakeoverData.cpp`)

The `TakeoverData` class contains all information needed for takeover:

**Capability Flags (`TakeoverCapabilities`):**

| Flag                        | Value   | Description                                       |
| --------------------------- | ------- | ------------------------------------------------- |
| `CUSTOM_SERIALIZATION`      | 1 << 0  | Deprecated custom format, no longer supported     |
| `FUSE`                      | 1 << 1  | Supports FUSE mount serialization                 |
| `THRIFT_SERIALIZATION`      | 1 << 2  | Uses Thrift for serialization (required)          |
| `PING`                      | 1 << 3  | Server pings client before sending data           |
| `MOUNT_TYPES`               | 1 << 4  | Protocol includes mount type information          |
| `NFS`                       | 1 << 5  | Supports NFS mount serialization                  |
| `RESULT_TYPE_SERIALIZATION` | 1 << 6  | Uses `SerializedTakeoverResult` format            |
| `ORDERED_FDS`               | 1 << 7  | File descriptor order is specified in message     |
| `OPTIONAL_MOUNTD`           | 1 << 8  | Mountd socket is optional (requires ORDERED_FDS)  |
| `CAPABILITY_MATCHING`       | 1 << 9  | Uses capability-based protocol negotiation        |
| `INCLUDE_HEADER_SIZE`       | 1 << 10 | Header includes its size for future extensibility |
| `CHUNKED_MESSAGE`           | 1 << 11 | Supports chunked message transfer for large data  |

**Supported Capabilities (current build):**

```cpp
FUSE | MOUNT_TYPES | PING | THRIFT_SERIALIZATION | NFS |
RESULT_TYPE_SERIALIZATION | ORDERED_FDS | OPTIONAL_MOUNTD |
CAPABILITY_MATCHING | INCLUDE_HEADER_SIZE | CHUNKED_MESSAGE
```

**Protocol Versions:**

| Version | Description                                                 |
| ------- | ----------------------------------------------------------- |
| 0       | Never supported (used for testing)                          |
| 1       | Deprecated: original protocol                               |
| 3       | Introduced Thrift serialization                             |
| 4       | Added ping handshake                                        |
| 5       | Added NFS mount support                                     |
| 6       | Added generic serialization and optional file descriptors   |
| 7       | Capability-based negotiation, header size, chunked messages |

Note: Version numbers are being phased out in favor of capability-based
negotiation. Version 7 should be the last numbered version. After this version,
server and client negotiate and choose the capabilities that both support.

**Data Members:**

- `lockFile` - The main eden lock file preventing multiple processes
- `thriftSocket` - The thrift server socket
- `mountdServerSocket` - Optional socket for NFS mountd
- `generalFDOrder` - Order of file descriptors in the message
- `mountPoints` - Vector of `MountInfo` for each mount
- `takeoverComplete` - Promise fulfilled when takeover data is sent

**MountInfo Structure:**

- `mountPath` - Absolute path where filesystem is mounted
- `stateDirectory` - Path to EdenFS state directory for this mount
- `channelInfo` - Variant containing either `FuseChannelData`, `NfsChannelData`,
  or `ProjFsChannelData`
- `inodeMap` - Serialized inode map data

**Serialization Format:**

The serialized message format is:

```
<32-bit version><32-bit header size><64-bit capabilities><thrift-serialized data>
```

- **version** - Protocol version for legacy compatibility
- **header size** - Size of the header (excluding version and size fields)
- **capabilities** - The agreed-upon capabilities bitmask
- **data** - Thrift-serialized `SerializedTakeoverResult`

For chunked transfers, data is split into chunks of up to 512 MB (configurable).

**Key Functions:**

- `serialize(capabilities, msg)` - Serialize takeover data into a UnixSocket
  message
- `deserialize(msg)` - Deserialize a UnixSocket message into TakeoverData
- `serializePing()` / `isPing(buf)` - Create/detect ping messages
- `serializeFirstChunk()` / `isFirstChunk(buf)` - Create/detect chunk markers
- `serializeLastChunk()` / `isLastChunk(buf)` - Create/detect chunk markers
- `computeCompatibleVersion(versions, supported)` - Find best compatible version
- `computeCompatibleCapabilities(capabilities, supported)` - Compute shared
  capabilities
- `versionToCapabilities(version)` - Convert version number to capability set
- `capabilitiesToVersion(capabilities)` - Convert capabilities to version number

### Handler (`TakeoverHandler.h`)

`TakeoverHandler` is a pure virtual interface for classes that want to implement
graceful takeover functionality. This is primarily implemented by the
`EdenServer` class. Alternative implementations exist for unit testing.

**Virtual Functions:**

- `startTakeoverShutdown()` - Called when a graceful shutdown has been
  requested, with a remote process attempting to take over the currently running
  mount points. Returns a `Future<TakeoverData>` that will produce the takeover
  data once the edenfs process is ready to transfer its mounts.

- `closeStorage()` - Called before sending the `TakeoverData` to the client,
  after a successful ready handshake (if applicable). This function should close
  storage used by the server (local stores, backing stores) to release locks so
  the new process can acquire them.

## Takeover Protocol Flow

```
Client (New EdenFS)                  Server (Old EdenFS)
        |                                       |
        |-------- Connect to socket ----------->|
        |                                       |
        |---- TakeoverVersionQuery ------------>|
        |     (versions, capabilities)          |
        |         timeout 5 seconds             |
        |                          [Validate UID matches]
        |                          [Negotiate protocol]
        |                          [startTakeoverShutdown()]
        |                                       |
        |<-------------- Ping ------------------|
        |                                       |
        |------------- Ping Response ---------->|
        |            timeout 5 seconds          |
        |                          [closeStorage()]
        |                                       |
        |<-------- FIRST_CHUNK (if chunked) ----|
        |                                       |
        |<-------- Data + File Descriptors -----|
        |<-------- More Data Chunks ... --------|
        |                                       |
        |<-------- LAST_CHUNK (if chunked) -----|
        |                                       |
        [Deserialize TakeoverData]              |
        [Take over mounts]                      |
        |                                       |
                                   [Fulfill takeoverComplete promise]
                                   [Exit gracefully]
```

Note: `takeover-receive-timeout` is a configurable flag (through `EdenConfig.h`)
defaulted to 2.5 minutes. This is the time that the client will wait for the
server to send the takeover data. This timeout applies to each chunk of data
when sending data in chunks.

## Error Handling and Recovery

The takeover system includes several mechanisms for handling errors:

1. **UID Validation** - Prevents unauthorized processes from taking over mounts

2. **Timeout Handling** - Various timeouts prevent hanging:
   - 5-second timeout for receiving version query
   - 5-second timeout for ping response (configurable)
   - Configurable timeout for receiving takeover data (default 5 minutes)

3. **Ping Verification** - Before sending takeover data, the server pings the
   client to ensure it's still responsive. If the client doesn't respond, the
   server can recover and continue running.

4. **Fault Injection** - The server supports fault injection points for testing:
   - `takeover.ping_receive` - Inject faults during ping handling
   - `takeover.error during send` - Simulate errors during data transfer

5. **Recovery Path** - If takeover fails after shutdown has started but before
   data is sent, the server can recover by using the `TakeoverData` returned
   through the `takeoverComplete` promise.
