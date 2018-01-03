# radixbuf

Radix tree based on plain buffers.

There are 2 plain buffers:

  - Radix buffer: One or more radix trees mapping keys to their IDs. Read and write by the library.
  - Key buffer: The source of truth of full keys. Read by the library, write by the application.

An ID of a key could be an offset, or other meaningful numbers understood by the function reading a key.
