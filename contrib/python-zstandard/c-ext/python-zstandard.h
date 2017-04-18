/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#define PY_SSIZE_T_CLEAN
#include <Python.h>
#include "structmember.h"

#define ZSTD_STATIC_LINKING_ONLY
#define ZDICT_STATIC_LINKING_ONLY
#include "mem.h"
#include "zstd.h"
#include "zdict.h"
#include "zstdmt_compress.h"

#define PYTHON_ZSTANDARD_VERSION "0.8.1"

typedef enum {
	compressorobj_flush_finish,
	compressorobj_flush_block,
} CompressorObj_Flush;

/*
   Represents a CompressionParameters type.

   This type is basically a wrapper around ZSTD_compressionParameters.
*/
typedef struct {
	PyObject_HEAD
	unsigned windowLog;
	unsigned chainLog;
	unsigned hashLog;
	unsigned searchLog;
	unsigned searchLength;
	unsigned targetLength;
	ZSTD_strategy strategy;
} CompressionParametersObject;

extern PyTypeObject CompressionParametersType;

/*
   Represents a FrameParameters type.

   This type is basically a wrapper around ZSTD_frameParams.
*/
typedef struct {
	PyObject_HEAD
	unsigned long long frameContentSize;
	unsigned windowSize;
	unsigned dictID;
	char checksumFlag;
} FrameParametersObject;

extern PyTypeObject FrameParametersType;

/*
   Represents a ZstdCompressionDict type.

   Instances hold data used for a zstd compression dictionary.
*/
typedef struct {
	PyObject_HEAD

	/* Pointer to dictionary data. Owned by self. */
	void* dictData;
	/* Size of dictionary data. */
	size_t dictSize;
	/* k parameter for cover dictionaries. Only populated by train_cover_dict(). */
	unsigned k;
	/* d parameter for cover dictionaries. Only populated by train_cover_dict(). */
	unsigned d;
} ZstdCompressionDict;

extern PyTypeObject ZstdCompressionDictType;

/*
   Represents a ZstdCompressor type.
*/
typedef struct {
	PyObject_HEAD

	/* Configured compression level. Should be always set. */
	int compressionLevel;
	/* Number of threads to use for operations. */
	unsigned int threads;
	/* Pointer to compression dictionary to use. NULL if not using dictionary
	   compression. */
	ZstdCompressionDict* dict;
	/* Compression context to use. Populated during object construction. NULL
	   if using multi-threaded compression. */
	ZSTD_CCtx* cctx;
	/* Multi-threaded compression context to use. Populated during object
	   construction. NULL if not using multi-threaded compression. */
	ZSTDMT_CCtx* mtcctx;
	/* Digest compression dictionary. NULL initially. Populated on first use. */
	ZSTD_CDict* cdict;
	/* Low-level compression parameter control. NULL unless passed to
	   constructor. Takes precedence over `compressionLevel` if defined. */
	CompressionParametersObject* cparams;
	/* Controls zstd frame options. */
	ZSTD_frameParameters fparams;
	/* Holds state for streaming compression. Shared across all invocation.
	   Populated on first use. */
	ZSTD_CStream* cstream;
} ZstdCompressor;

extern PyTypeObject ZstdCompressorType;

typedef struct {
	PyObject_HEAD

	ZstdCompressor* compressor;
	ZSTD_outBuffer output;
	int finished;
} ZstdCompressionObj;

extern PyTypeObject ZstdCompressionObjType;

typedef struct {
	PyObject_HEAD

	ZstdCompressor* compressor;
	PyObject* writer;
	Py_ssize_t sourceSize;
	size_t outSize;
	int entered;
} ZstdCompressionWriter;

extern PyTypeObject ZstdCompressionWriterType;

typedef struct {
	PyObject_HEAD

	ZstdCompressor* compressor;
	PyObject* reader;
	Py_buffer* buffer;
	Py_ssize_t bufferOffset;
	Py_ssize_t sourceSize;
	size_t inSize;
	size_t outSize;

	ZSTD_inBuffer input;
	ZSTD_outBuffer output;
	int finishedOutput;
	int finishedInput;
	PyObject* readResult;
} ZstdCompressorIterator;

extern PyTypeObject ZstdCompressorIteratorType;

typedef struct {
	PyObject_HEAD

	ZSTD_DCtx* dctx;

	ZstdCompressionDict* dict;
	ZSTD_DDict* ddict;
	ZSTD_DStream* dstream;
} ZstdDecompressor;

extern PyTypeObject ZstdDecompressorType;

typedef struct {
	PyObject_HEAD

	ZstdDecompressor* decompressor;
	int finished;
} ZstdDecompressionObj;

extern PyTypeObject ZstdDecompressionObjType;

typedef struct {
	PyObject_HEAD

	ZstdDecompressor* decompressor;
	PyObject* writer;
	size_t outSize;
	int entered;
} ZstdDecompressionWriter;

extern PyTypeObject ZstdDecompressionWriterType;

typedef struct {
	PyObject_HEAD

	ZstdDecompressor* decompressor;
	PyObject* reader;
	Py_buffer* buffer;
	Py_ssize_t bufferOffset;
	size_t inSize;
	size_t outSize;
	size_t skipBytes;
	ZSTD_inBuffer input;
	ZSTD_outBuffer output;
	Py_ssize_t readCount;
	int finishedInput;
	int finishedOutput;
} ZstdDecompressorIterator;

extern PyTypeObject ZstdDecompressorIteratorType;

typedef struct {
	int errored;
	PyObject* chunk;
} DecompressorIteratorResult;

typedef struct {
	unsigned long long offset;
	unsigned long long length;
} BufferSegment;

typedef struct {
	PyObject_HEAD

	PyObject* parent;
	BufferSegment* segments;
	Py_ssize_t segmentCount;
} ZstdBufferSegments;

extern PyTypeObject ZstdBufferSegmentsType;

typedef struct {
	PyObject_HEAD

	PyObject* parent;
	void* data;
	Py_ssize_t dataSize;
	unsigned long long offset;
} ZstdBufferSegment;

extern PyTypeObject ZstdBufferSegmentType;

typedef struct {
	PyObject_HEAD

	Py_buffer parent;
	void* data;
	unsigned long long dataSize;
	BufferSegment* segments;
	Py_ssize_t segmentCount;
	int useFree;
} ZstdBufferWithSegments;

extern PyTypeObject ZstdBufferWithSegmentsType;

/**
 * An ordered collection of BufferWithSegments exposed as a squashed collection.
 *
 * This type provides a virtual view spanning multiple BufferWithSegments
 * instances. It allows multiple instances to be "chained" together and
 * exposed as a single collection. e.g. if there are 2 buffers holding
 * 10 segments each, then o[14] will access the 5th segment in the 2nd buffer.
 */
typedef struct {
	PyObject_HEAD

	/* An array of buffers that should be exposed through this instance. */
	ZstdBufferWithSegments** buffers;
	/* Number of elements in buffers array. */
	Py_ssize_t bufferCount;
	/* Array of first offset in each buffer instance. 0th entry corresponds
	   to number of elements in the 0th buffer. 1st entry corresponds to the
	   sum of elements in 0th and 1st buffers. */
	Py_ssize_t* firstElements;
} ZstdBufferWithSegmentsCollection;

extern PyTypeObject ZstdBufferWithSegmentsCollectionType;

void ztopy_compression_parameters(CompressionParametersObject* params, ZSTD_compressionParameters* zparams);
CompressionParametersObject* get_compression_parameters(PyObject* self, PyObject* args);
FrameParametersObject* get_frame_parameters(PyObject* self, PyObject* args);
PyObject* estimate_compression_context_size(PyObject* self, PyObject* args);
int init_cstream(ZstdCompressor* compressor, unsigned long long sourceSize);
int init_mtcstream(ZstdCompressor* compressor, Py_ssize_t sourceSize);
int init_dstream(ZstdDecompressor* decompressor);
ZstdCompressionDict* train_dictionary(PyObject* self, PyObject* args, PyObject* kwargs);
ZstdCompressionDict* train_cover_dictionary(PyObject* self, PyObject* args, PyObject* kwargs);
ZstdBufferWithSegments* BufferWithSegments_FromMemory(void* data, unsigned long long dataSize, BufferSegment* segments, Py_ssize_t segmentsSize);
Py_ssize_t BufferWithSegmentsCollection_length(ZstdBufferWithSegmentsCollection*);
int cpu_count(void);
size_t roundpow2(size_t);
