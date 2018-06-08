/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"
#include "pool.h"

extern PyObject* ZstdError;

/**
  * Ensure the ZSTD_DStream on a ZstdDecompressor is initialized and reset.
  *
  * This should be called before starting a decompression operation with a
  * ZSTD_DStream on a ZstdDecompressor.
  */
int init_dstream(ZstdDecompressor* decompressor) {
	void* dictData = NULL;
	size_t dictSize = 0;
	size_t zresult;

	/* Simple case of dstream already exists. Just reset it. */
	if (decompressor->dstream) {
		zresult = ZSTD_resetDStream(decompressor->dstream);
		if (ZSTD_isError(zresult)) {
			PyErr_Format(ZstdError, "could not reset DStream: %s",
				ZSTD_getErrorName(zresult));
			return -1;
		}

		return 0;
	}

	decompressor->dstream = ZSTD_createDStream();
	if (!decompressor->dstream) {
		PyErr_SetString(ZstdError, "could not create DStream");
		return -1;
	}

	if (decompressor->dict) {
		dictData = decompressor->dict->dictData;
		dictSize = decompressor->dict->dictSize;
	}

	if (dictData) {
		zresult = ZSTD_initDStream_usingDict(decompressor->dstream, dictData, dictSize);
	}
	else {
		zresult = ZSTD_initDStream(decompressor->dstream);
	}

	if (ZSTD_isError(zresult)) {
		/* Don't leave a reference to an invalid object. */
		ZSTD_freeDStream(decompressor->dstream);
		decompressor->dstream = NULL;

		PyErr_Format(ZstdError, "could not initialize DStream: %s",
			ZSTD_getErrorName(zresult));
		return -1;
	}

	return 0;
}

PyDoc_STRVAR(Decompressor__doc__,
"ZstdDecompressor(dict_data=None)\n"
"\n"
"Create an object used to perform Zstandard decompression.\n"
"\n"
"An instance can perform multiple decompression operations."
);

static int Decompressor_init(ZstdDecompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"dict_data",
		NULL
	};

	ZstdCompressionDict* dict = NULL;

	self->dctx = NULL;
	self->dict = NULL;
	self->ddict = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "|O!:ZstdDecompressor", kwlist,
		&ZstdCompressionDictType, &dict)) {
		return -1;
	}

	/* TODO lazily initialize the reference ZSTD_DCtx on first use since
	   not instances of ZstdDecompressor will use a ZSTD_DCtx. */
	self->dctx = ZSTD_createDCtx();
	if (!self->dctx) {
		PyErr_NoMemory();
		goto except;
	}

	if (dict) {
		self->dict = dict;
		Py_INCREF(dict);
	}

	return 0;

except:
	if (self->dctx) {
		ZSTD_freeDCtx(self->dctx);
		self->dctx = NULL;
	}

	return -1;
}

static void Decompressor_dealloc(ZstdDecompressor* self) {
	Py_CLEAR(self->dict);

	if (self->ddict) {
		ZSTD_freeDDict(self->ddict);
		self->ddict = NULL;
	}

	if (self->dstream) {
		ZSTD_freeDStream(self->dstream);
		self->dstream = NULL;
	}

	if (self->dctx) {
		ZSTD_freeDCtx(self->dctx);
		self->dctx = NULL;
	}

	PyObject_Del(self);
}

PyDoc_STRVAR(Decompressor_copy_stream__doc__,
	"copy_stream(ifh, ofh[, read_size=default, write_size=default]) -- decompress data between streams\n"
	"\n"
	"Compressed data will be read from ``ifh``, decompressed, and written to\n"
	"``ofh``. ``ifh`` must have a ``read(size)`` method. ``ofh`` must have a\n"
	"``write(data)`` method.\n"
	"\n"
	"The optional ``read_size`` and ``write_size`` arguments control the chunk\n"
	"size of data that is ``read()`` and ``write()`` between streams. They default\n"
	"to the default input and output sizes of zstd decompressor streams.\n"
);

static PyObject* Decompressor_copy_stream(ZstdDecompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"ifh",
		"ofh",
		"read_size",
		"write_size",
		NULL
	};

	PyObject* source;
	PyObject* dest;
	size_t inSize = ZSTD_DStreamInSize();
	size_t outSize = ZSTD_DStreamOutSize();
	ZSTD_inBuffer input;
	ZSTD_outBuffer output;
	Py_ssize_t totalRead = 0;
	Py_ssize_t totalWrite = 0;
	char* readBuffer;
	Py_ssize_t readSize;
	PyObject* readResult;
	PyObject* res = NULL;
	size_t zresult = 0;
	PyObject* writeResult;
	PyObject* totalReadPy;
	PyObject* totalWritePy;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "OO|kk:copy_stream", kwlist,
		&source, &dest, &inSize, &outSize)) {
		return NULL;
	}

	if (!PyObject_HasAttrString(source, "read")) {
		PyErr_SetString(PyExc_ValueError, "first argument must have a read() method");
		return NULL;
	}

	if (!PyObject_HasAttrString(dest, "write")) {
		PyErr_SetString(PyExc_ValueError, "second argument must have a write() method");
		return NULL;
	}

	/* Prevent free on uninitialized memory in finally. */
	output.dst = NULL;

	if (0 != init_dstream(self)) {
		res = NULL;
		goto finally;
	}

	output.dst = PyMem_Malloc(outSize);
	if (!output.dst) {
		PyErr_NoMemory();
		res = NULL;
		goto finally;
	}
	output.size = outSize;
	output.pos = 0;

	/* Read source stream until EOF */
	while (1) {
		readResult = PyObject_CallMethod(source, "read", "n", inSize);
		if (!readResult) {
			PyErr_SetString(ZstdError, "could not read() from source");
			goto finally;
		}

		PyBytes_AsStringAndSize(readResult, &readBuffer, &readSize);

		/* If no data was read, we're at EOF. */
		if (0 == readSize) {
			break;
		}

		totalRead += readSize;

		/* Send data to decompressor */
		input.src = readBuffer;
		input.size = readSize;
		input.pos = 0;

		while (input.pos < input.size) {
			Py_BEGIN_ALLOW_THREADS
			zresult = ZSTD_decompressStream(self->dstream, &output, &input);
			Py_END_ALLOW_THREADS

			if (ZSTD_isError(zresult)) {
				PyErr_Format(ZstdError, "zstd decompressor error: %s",
					ZSTD_getErrorName(zresult));
				res = NULL;
				goto finally;
			}

			if (output.pos) {
#if PY_MAJOR_VERSION >= 3
				writeResult = PyObject_CallMethod(dest, "write", "y#",
#else
				writeResult = PyObject_CallMethod(dest, "write", "s#",
#endif
					output.dst, output.pos);

				Py_XDECREF(writeResult);
				totalWrite += output.pos;
				output.pos = 0;
			}
		}
	}

	/* Source stream is exhausted. Finish up. */

	totalReadPy = PyLong_FromSsize_t(totalRead);
	totalWritePy = PyLong_FromSsize_t(totalWrite);
	res = PyTuple_Pack(2, totalReadPy, totalWritePy);
	Py_DECREF(totalReadPy);
	Py_DECREF(totalWritePy);

finally:
	if (output.dst) {
		PyMem_Free(output.dst);
	}

	return res;
}

PyDoc_STRVAR(Decompressor_decompress__doc__,
"decompress(data[, max_output_size=None]) -- Decompress data in its entirety\n"
"\n"
"This method will decompress the entirety of the argument and return the\n"
"result.\n"
"\n"
"The input bytes are expected to contain a full Zstandard frame (something\n"
"compressed with ``ZstdCompressor.compress()`` or similar). If the input does\n"
"not contain a full frame, an exception will be raised.\n"
"\n"
"If the frame header of the compressed data does not contain the content size\n"
"``max_output_size`` must be specified or ``ZstdError`` will be raised. An\n"
"allocation of size ``max_output_size`` will be performed and an attempt will\n"
"be made to perform decompression into that buffer. If the buffer is too\n"
"small or cannot be allocated, ``ZstdError`` will be raised. The buffer will\n"
"be resized if it is too large.\n"
"\n"
"Uncompressed data could be much larger than compressed data. As a result,\n"
"calling this function could result in a very large memory allocation being\n"
"performed to hold the uncompressed data. Therefore it is **highly**\n"
"recommended to use a streaming decompression method instead of this one.\n"
);

PyObject* Decompressor_decompress(ZstdDecompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"data",
		"max_output_size",
		NULL
	};

	const char* source;
	Py_ssize_t sourceSize;
	Py_ssize_t maxOutputSize = 0;
	unsigned long long decompressedSize;
	size_t destCapacity;
	PyObject* result = NULL;
	void* dictData = NULL;
	size_t dictSize = 0;
	size_t zresult;

#if PY_MAJOR_VERSION >= 3
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "y#|n:decompress",
#else
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "s#|n:decompress",
#endif
		kwlist, &source, &sourceSize, &maxOutputSize)) {
		return NULL;
	}

	if (self->dict) {
		dictData = self->dict->dictData;
		dictSize = self->dict->dictSize;
	}

	if (dictData && !self->ddict) {
		Py_BEGIN_ALLOW_THREADS
		self->ddict = ZSTD_createDDict_byReference(dictData, dictSize);
		Py_END_ALLOW_THREADS

		if (!self->ddict) {
			PyErr_SetString(ZstdError, "could not create decompression dict");
			return NULL;
		}
	}

	decompressedSize = ZSTD_getDecompressedSize(source, sourceSize);
	/* 0 returned if content size not in the zstd frame header */
	if (0 == decompressedSize) {
		if (0 == maxOutputSize) {
			PyErr_SetString(ZstdError, "input data invalid or missing content size "
				"in frame header");
			return NULL;
		}
		else {
			result = PyBytes_FromStringAndSize(NULL, maxOutputSize);
			destCapacity = maxOutputSize;
		}
	}
	else {
		result = PyBytes_FromStringAndSize(NULL, decompressedSize);
		destCapacity = decompressedSize;
	}

	if (!result) {
		return NULL;
	}

	Py_BEGIN_ALLOW_THREADS
	if (self->ddict) {
		zresult = ZSTD_decompress_usingDDict(self->dctx,
			PyBytes_AsString(result), destCapacity,
			source, sourceSize, self->ddict);
	}
	else {
		zresult = ZSTD_decompressDCtx(self->dctx,
			PyBytes_AsString(result), destCapacity, source, sourceSize);
	}
	Py_END_ALLOW_THREADS

	if (ZSTD_isError(zresult)) {
		PyErr_Format(ZstdError, "decompression error: %s", ZSTD_getErrorName(zresult));
		Py_DECREF(result);
		return NULL;
	}
	else if (decompressedSize && zresult != decompressedSize) {
		PyErr_Format(ZstdError, "decompression error: decompressed %zu bytes; expected %llu",
			zresult, decompressedSize);
		Py_DECREF(result);
		return NULL;
	}
	else if (zresult < destCapacity) {
		if (_PyBytes_Resize(&result, zresult)) {
			Py_DECREF(result);
			return NULL;
		}
	}

	return result;
}

PyDoc_STRVAR(Decompressor_decompressobj__doc__,
"decompressobj()\n"
"\n"
"Incrementally feed data into a decompressor.\n"
"\n"
"The returned object exposes a ``decompress(data)`` method. This makes it\n"
"compatible with ``zlib.decompressobj`` and ``bz2.BZ2Decompressor`` so that\n"
"callers can swap in the zstd decompressor while using the same API.\n"
);

static ZstdDecompressionObj* Decompressor_decompressobj(ZstdDecompressor* self) {
	ZstdDecompressionObj* result = (ZstdDecompressionObj*)PyObject_CallObject((PyObject*)&ZstdDecompressionObjType, NULL);
	if (!result) {
		return NULL;
	}

	if (0 != init_dstream(self)) {
		Py_DECREF(result);
		return NULL;
	}

	result->decompressor = self;
	Py_INCREF(result->decompressor);

	return result;
}

PyDoc_STRVAR(Decompressor_read_from__doc__,
"read_from(reader[, read_size=default, write_size=default, skip_bytes=0])\n"
"Read compressed data and return an iterator\n"
"\n"
"Returns an iterator of decompressed data chunks produced from reading from\n"
"the ``reader``.\n"
"\n"
"Compressed data will be obtained from ``reader`` by calling the\n"
"``read(size)`` method of it. The source data will be streamed into a\n"
"decompressor. As decompressed data is available, it will be exposed to the\n"
"returned iterator.\n"
"\n"
"Data is ``read()`` in chunks of size ``read_size`` and exposed to the\n"
"iterator in chunks of size ``write_size``. The default values are the input\n"
"and output sizes for a zstd streaming decompressor.\n"
"\n"
"There is also support for skipping the first ``skip_bytes`` of data from\n"
"the source.\n"
);

static ZstdDecompressorIterator* Decompressor_read_from(ZstdDecompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"reader",
		"read_size",
		"write_size",
		"skip_bytes",
		NULL
	};

	PyObject* reader;
	size_t inSize = ZSTD_DStreamInSize();
	size_t outSize = ZSTD_DStreamOutSize();
	ZstdDecompressorIterator* result;
	size_t skipBytes = 0;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|kkk:read_from", kwlist,
		&reader, &inSize, &outSize, &skipBytes)) {
		return NULL;
	}

	if (skipBytes >= inSize) {
		PyErr_SetString(PyExc_ValueError,
			"skip_bytes must be smaller than read_size");
		return NULL;
	}

	result = (ZstdDecompressorIterator*)PyObject_CallObject((PyObject*)&ZstdDecompressorIteratorType, NULL);
	if (!result) {
		return NULL;
	}

	if (PyObject_HasAttrString(reader, "read")) {
		result->reader = reader;
		Py_INCREF(result->reader);
	}
	else if (1 == PyObject_CheckBuffer(reader)) {
		/* Object claims it is a buffer. Try to get a handle to it. */
		result->buffer = PyMem_Malloc(sizeof(Py_buffer));
		if (!result->buffer) {
			goto except;
		}

		memset(result->buffer, 0, sizeof(Py_buffer));

		if (0 != PyObject_GetBuffer(reader, result->buffer, PyBUF_CONTIG_RO)) {
			goto except;
		}
	}
	else {
		PyErr_SetString(PyExc_ValueError,
			"must pass an object with a read() method or conforms to buffer protocol");
		goto except;
	}

	result->decompressor = self;
	Py_INCREF(result->decompressor);

	result->inSize = inSize;
	result->outSize = outSize;
	result->skipBytes = skipBytes;

	if (0 != init_dstream(self)) {
		goto except;
	}

	result->input.src = PyMem_Malloc(inSize);
	if (!result->input.src) {
		PyErr_NoMemory();
		goto except;
	}

	goto finally;

except:
	Py_CLEAR(result->reader);

	if (result->buffer) {
		PyBuffer_Release(result->buffer);
		Py_CLEAR(result->buffer);
	}

	Py_CLEAR(result);

finally:

	return result;
}

PyDoc_STRVAR(Decompressor_write_to__doc__,
"Create a context manager to write decompressed data to an object.\n"
"\n"
"The passed object must have a ``write()`` method.\n"
"\n"
"The caller feeds intput data to the object by calling ``write(data)``.\n"
"Decompressed data is written to the argument given as it is decompressed.\n"
"\n"
"An optional ``write_size`` argument defines the size of chunks to\n"
"``write()`` to the writer. It defaults to the default output size for a zstd\n"
"streaming decompressor.\n"
);

static ZstdDecompressionWriter* Decompressor_write_to(ZstdDecompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"writer",
		"write_size",
		NULL
	};

	PyObject* writer;
	size_t outSize = ZSTD_DStreamOutSize();
	ZstdDecompressionWriter* result;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|k:write_to", kwlist,
		&writer, &outSize)) {
		return NULL;
	}

	if (!PyObject_HasAttrString(writer, "write")) {
		PyErr_SetString(PyExc_ValueError, "must pass an object with a write() method");
		return NULL;
	}

	result = (ZstdDecompressionWriter*)PyObject_CallObject((PyObject*)&ZstdDecompressionWriterType, NULL);
	if (!result) {
		return NULL;
	}

	result->decompressor = self;
	Py_INCREF(result->decompressor);

	result->writer = writer;
	Py_INCREF(result->writer);

	result->outSize = outSize;

	return result;
}

PyDoc_STRVAR(Decompressor_decompress_content_dict_chain__doc__,
"Decompress a series of chunks using the content dictionary chaining technique\n"
);

static PyObject* Decompressor_decompress_content_dict_chain(PyObject* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"frames",
		NULL
	};

	PyObject* chunks;
	Py_ssize_t chunksLen;
	Py_ssize_t chunkIndex;
	char parity = 0;
	PyObject* chunk;
	char* chunkData;
	Py_ssize_t chunkSize;
	ZSTD_DCtx* dctx = NULL;
	size_t zresult;
	ZSTD_frameParams frameParams;
	void* buffer1 = NULL;
	size_t buffer1Size = 0;
	size_t buffer1ContentSize = 0;
	void* buffer2 = NULL;
	size_t buffer2Size = 0;
	size_t buffer2ContentSize = 0;
	void* destBuffer = NULL;
	PyObject* result = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O!:decompress_content_dict_chain",
		kwlist, &PyList_Type, &chunks)) {
		return NULL;
	}

	chunksLen = PyList_Size(chunks);
	if (!chunksLen) {
		PyErr_SetString(PyExc_ValueError, "empty input chain");
		return NULL;
	}

	/* The first chunk should not be using a dictionary. We handle it specially. */
	chunk = PyList_GetItem(chunks, 0);
	if (!PyBytes_Check(chunk)) {
		PyErr_SetString(PyExc_ValueError, "chunk 0 must be bytes");
		return NULL;
	}

	/* We require that all chunks be zstd frames and that they have content size set. */
	PyBytes_AsStringAndSize(chunk, &chunkData, &chunkSize);
	zresult = ZSTD_getFrameParams(&frameParams, (void*)chunkData, chunkSize);
	if (ZSTD_isError(zresult)) {
		PyErr_SetString(PyExc_ValueError, "chunk 0 is not a valid zstd frame");
		return NULL;
	}
	else if (zresult) {
		PyErr_SetString(PyExc_ValueError, "chunk 0 is too small to contain a zstd frame");
		return NULL;
	}

	if (0 == frameParams.frameContentSize) {
		PyErr_SetString(PyExc_ValueError, "chunk 0 missing content size in frame");
		return NULL;
	}

	dctx = ZSTD_createDCtx();
	if (!dctx) {
		PyErr_NoMemory();
		goto finally;
	}

	buffer1Size = frameParams.frameContentSize;
	buffer1 = PyMem_Malloc(buffer1Size);
	if (!buffer1) {
		goto finally;
	}

	Py_BEGIN_ALLOW_THREADS
	zresult = ZSTD_decompressDCtx(dctx, buffer1, buffer1Size, chunkData, chunkSize);
	Py_END_ALLOW_THREADS
	if (ZSTD_isError(zresult)) {
		PyErr_Format(ZstdError, "could not decompress chunk 0: %s", ZSTD_getErrorName(zresult));
		goto finally;
	}

	buffer1ContentSize = zresult;

	/* Special case of a simple chain. */
	if (1 == chunksLen) {
		result = PyBytes_FromStringAndSize(buffer1, buffer1Size);
		goto finally;
	}

	/* This should ideally look at next chunk. But this is slightly simpler. */
	buffer2Size = frameParams.frameContentSize;
	buffer2 = PyMem_Malloc(buffer2Size);
	if (!buffer2) {
		goto finally;
	}

	/* For each subsequent chunk, use the previous fulltext as a content dictionary.
	   Our strategy is to have 2 buffers. One holds the previous fulltext (to be
	   used as a content dictionary) and the other holds the new fulltext. The
	   buffers grow when needed but never decrease in size. This limits the
	   memory allocator overhead.
	*/
	for (chunkIndex = 1; chunkIndex < chunksLen; chunkIndex++) {
		chunk = PyList_GetItem(chunks, chunkIndex);
		if (!PyBytes_Check(chunk)) {
			PyErr_Format(PyExc_ValueError, "chunk %zd must be bytes", chunkIndex);
			goto finally;
		}

		PyBytes_AsStringAndSize(chunk, &chunkData, &chunkSize);
		zresult = ZSTD_getFrameParams(&frameParams, (void*)chunkData, chunkSize);
		if (ZSTD_isError(zresult)) {
			PyErr_Format(PyExc_ValueError, "chunk %zd is not a valid zstd frame", chunkIndex);
			goto finally;
		}
		else if (zresult) {
			PyErr_Format(PyExc_ValueError, "chunk %zd is too small to contain a zstd frame", chunkIndex);
			goto finally;
		}

		if (0 == frameParams.frameContentSize) {
			PyErr_Format(PyExc_ValueError, "chunk %zd missing content size in frame", chunkIndex);
			goto finally;
		}

		parity = chunkIndex % 2;

		/* This could definitely be abstracted to reduce code duplication. */
		if (parity) {
			/* Resize destination buffer to hold larger content. */
			if (buffer2Size < frameParams.frameContentSize) {
				buffer2Size = frameParams.frameContentSize;
				destBuffer = PyMem_Realloc(buffer2, buffer2Size);
				if (!destBuffer) {
					goto finally;
				}
				buffer2 = destBuffer;
			}

			Py_BEGIN_ALLOW_THREADS
			zresult = ZSTD_decompress_usingDict(dctx, buffer2, buffer2Size,
				chunkData, chunkSize, buffer1, buffer1ContentSize);
			Py_END_ALLOW_THREADS
			if (ZSTD_isError(zresult)) {
				PyErr_Format(ZstdError, "could not decompress chunk %zd: %s",
					chunkIndex, ZSTD_getErrorName(zresult));
				goto finally;
			}
			buffer2ContentSize = zresult;
		}
		else {
			if (buffer1Size < frameParams.frameContentSize) {
				buffer1Size = frameParams.frameContentSize;
				destBuffer = PyMem_Realloc(buffer1, buffer1Size);
				if (!destBuffer) {
					goto finally;
				}
				buffer1 = destBuffer;
			}

			Py_BEGIN_ALLOW_THREADS
			zresult = ZSTD_decompress_usingDict(dctx, buffer1, buffer1Size,
				chunkData, chunkSize, buffer2, buffer2ContentSize);
			Py_END_ALLOW_THREADS
			if (ZSTD_isError(zresult)) {
				PyErr_Format(ZstdError, "could not decompress chunk %zd: %s",
					chunkIndex, ZSTD_getErrorName(zresult));
				goto finally;
			}
			buffer1ContentSize = zresult;
		}
	}

	result = PyBytes_FromStringAndSize(parity ? buffer2 : buffer1,
		parity ? buffer2ContentSize : buffer1ContentSize);

finally:
	if (buffer2) {
		PyMem_Free(buffer2);
	}
	if (buffer1) {
		PyMem_Free(buffer1);
	}

	if (dctx) {
		ZSTD_freeDCtx(dctx);
	}

	return result;
}

typedef struct {
	void* sourceData;
	size_t sourceSize;
	unsigned long long destSize;
} FramePointer;

typedef struct {
	FramePointer* frames;
	Py_ssize_t framesSize;
	unsigned long long compressedSize;
} FrameSources;

typedef struct {
	void* dest;
	Py_ssize_t destSize;
	BufferSegment* segments;
	Py_ssize_t segmentsSize;
} DestBuffer;

typedef enum {
	WorkerError_none = 0,
	WorkerError_zstd = 1,
	WorkerError_memory = 2,
	WorkerError_sizeMismatch = 3,
	WorkerError_unknownSize = 4,
} WorkerError;

typedef struct {
	/* Source records and length */
	FramePointer* framePointers;
	/* Which records to process. */
	Py_ssize_t startOffset;
	Py_ssize_t endOffset;
	unsigned long long totalSourceSize;

	/* Compression state and settings. */
	ZSTD_DCtx* dctx;
	ZSTD_DDict* ddict;
	int requireOutputSizes;

	/* Output storage. */
	DestBuffer* destBuffers;
	Py_ssize_t destCount;

	/* Item that error occurred on. */
	Py_ssize_t errorOffset;
	/* If an error occurred. */
	WorkerError error;
	/* result from zstd decompression operation */
	size_t zresult;
} WorkerState;

static void decompress_worker(WorkerState* state) {
	size_t allocationSize;
	DestBuffer* destBuffer;
	Py_ssize_t frameIndex;
	Py_ssize_t localOffset = 0;
	Py_ssize_t currentBufferStartIndex = state->startOffset;
	Py_ssize_t remainingItems = state->endOffset - state->startOffset + 1;
	void* tmpBuf;
	Py_ssize_t destOffset = 0;
	FramePointer* framePointers = state->framePointers;
	size_t zresult;
	unsigned long long totalOutputSize = 0;

	assert(NULL == state->destBuffers);
	assert(0 == state->destCount);
	assert(state->endOffset - state->startOffset >= 0);

	/*
	 * We need to allocate a buffer to hold decompressed data. How we do this
	 * depends on what we know about the output. The following scenarios are
	 * possible:
	 *
	 * 1. All structs defining frames declare the output size.
	 * 2. The decompressed size is embedded within the zstd frame.
	 * 3. The decompressed size is not stored anywhere.
	 *
	 * For now, we only support #1 and #2.
	 */

	/* Resolve ouput segments. */
	for (frameIndex = state->startOffset; frameIndex <= state->endOffset; frameIndex++) {
		FramePointer* fp = &framePointers[frameIndex];

		if (0 == fp->destSize) {
			fp->destSize = ZSTD_getDecompressedSize(fp->sourceData, fp->sourceSize);
			if (0 == fp->destSize && state->requireOutputSizes) {
				state->error = WorkerError_unknownSize;
				state->errorOffset = frameIndex;
				return;
			}
		}

		totalOutputSize += fp->destSize;
	}

	state->destBuffers = calloc(1, sizeof(DestBuffer));
	if (NULL == state->destBuffers) {
		state->error = WorkerError_memory;
		return;
	}

	state->destCount = 1;

	destBuffer = &state->destBuffers[state->destCount - 1];

	assert(framePointers[state->startOffset].destSize > 0); /* For now. */

	allocationSize = roundpow2(state->totalSourceSize);

	if (framePointers[state->startOffset].destSize > allocationSize) {
		allocationSize = roundpow2(framePointers[state->startOffset].destSize);
	}

	destBuffer->dest = malloc(allocationSize);
	if (NULL == destBuffer->dest) {
		state->error = WorkerError_memory;
		return;
	}

	destBuffer->destSize = allocationSize;

	destBuffer->segments = calloc(remainingItems, sizeof(BufferSegment));
	if (NULL == destBuffer->segments) {
		/* Caller will free state->dest as part of cleanup. */
		state->error = WorkerError_memory;
		return;
	}

	destBuffer->segmentsSize = remainingItems;

	for (frameIndex = state->startOffset; frameIndex <= state->endOffset; frameIndex++) {
		const void* source = framePointers[frameIndex].sourceData;
		const size_t sourceSize = framePointers[frameIndex].sourceSize;
		void* dest;
		const size_t decompressedSize = framePointers[frameIndex].destSize;
		size_t destAvailable = destBuffer->destSize - destOffset;

		assert(decompressedSize > 0); /* For now. */

		/*
		 * Not enough space in current buffer. Finish current before and allocate and
		 * switch to a new one.
		 */
		if (decompressedSize > destAvailable) {
			/*
			 * Shrinking the destination buffer is optional. But it should be cheap,
			 * so we just do it.
			 */
			if (destAvailable) {
				tmpBuf = realloc(destBuffer->dest, destOffset);
				if (NULL == tmpBuf) {
					state->error = WorkerError_memory;
					return;
				}

				destBuffer->dest = tmpBuf;
				destBuffer->destSize = destOffset;
			}

			/* Truncate segments buffer. */
			tmpBuf = realloc(destBuffer->segments,
				(frameIndex - currentBufferStartIndex) * sizeof(BufferSegment));
			if (NULL == tmpBuf) {
				state->error = WorkerError_memory;
				return;
			}

			destBuffer->segments = tmpBuf;
			destBuffer->segmentsSize = frameIndex - currentBufferStartIndex;

			/* Grow space for new DestBuffer. */
			tmpBuf = realloc(state->destBuffers, (state->destCount + 1) * sizeof(DestBuffer));
			if (NULL == tmpBuf) {
				state->error = WorkerError_memory;
				return;
			}

			state->destBuffers = tmpBuf;
			state->destCount++;

			destBuffer = &state->destBuffers[state->destCount - 1];

			/* Don't take any chances will non-NULL pointers. */
			memset(destBuffer, 0, sizeof(DestBuffer));

			allocationSize = roundpow2(state->totalSourceSize);

			if (decompressedSize > allocationSize) {
				allocationSize = roundpow2(decompressedSize);
			}

			destBuffer->dest = malloc(allocationSize);
			if (NULL == destBuffer->dest) {
				state->error = WorkerError_memory;
				return;
			}

			destBuffer->destSize = allocationSize;
			destAvailable = allocationSize;
			destOffset = 0;
			localOffset = 0;

			destBuffer->segments = calloc(remainingItems, sizeof(BufferSegment));
			if (NULL == destBuffer->segments) {
				state->error = WorkerError_memory;
				return;
			}

			destBuffer->segmentsSize = remainingItems;
			currentBufferStartIndex = frameIndex;
		}

		dest = (char*)destBuffer->dest + destOffset;

		if (state->ddict) {
			zresult = ZSTD_decompress_usingDDict(state->dctx, dest, decompressedSize,
				source, sourceSize, state->ddict);
		}
		else {
			zresult = ZSTD_decompressDCtx(state->dctx, dest, decompressedSize,
				source, sourceSize);
		}

		if (ZSTD_isError(zresult)) {
			state->error = WorkerError_zstd;
			state->zresult = zresult;
			state->errorOffset = frameIndex;
			return;
		}
		else if (zresult != decompressedSize) {
			state->error = WorkerError_sizeMismatch;
			state->zresult = zresult;
			state->errorOffset = frameIndex;
			return;
		}

		destBuffer->segments[localOffset].offset = destOffset;
		destBuffer->segments[localOffset].length = decompressedSize;
		destOffset += zresult;
		localOffset++;
		remainingItems--;
	}

	if (destBuffer->destSize > destOffset) {
		tmpBuf = realloc(destBuffer->dest, destOffset);
		if (NULL == tmpBuf) {
			state->error = WorkerError_memory;
			return;
		}

		destBuffer->dest = tmpBuf;
		destBuffer->destSize = destOffset;
	}
}

ZstdBufferWithSegmentsCollection* decompress_from_framesources(ZstdDecompressor* decompressor, FrameSources* frames,
	unsigned int threadCount) {
	void* dictData = NULL;
	size_t dictSize = 0;
	Py_ssize_t i = 0;
	int errored = 0;
	Py_ssize_t segmentsCount;
	ZstdBufferWithSegments* bws = NULL;
	PyObject* resultArg = NULL;
	Py_ssize_t resultIndex;
	ZstdBufferWithSegmentsCollection* result = NULL;
	FramePointer* framePointers = frames->frames;
	unsigned long long workerBytes = 0;
	int currentThread = 0;
	Py_ssize_t workerStartOffset = 0;
	POOL_ctx* pool = NULL;
	WorkerState* workerStates = NULL;
	unsigned long long bytesPerWorker;

	/* Caller should normalize 0 and negative values to 1 or larger. */
	assert(threadCount >= 1);

	/* More threads than inputs makes no sense under any conditions. */
	threadCount = frames->framesSize < threadCount ? (unsigned int)frames->framesSize
												   : threadCount;

	/* TODO lower thread count if input size is too small and threads would just
	   add overhead. */

	if (decompressor->dict) {
		dictData = decompressor->dict->dictData;
		dictSize = decompressor->dict->dictSize;
	}

	if (dictData && !decompressor->ddict) {
		Py_BEGIN_ALLOW_THREADS
		decompressor->ddict = ZSTD_createDDict_byReference(dictData, dictSize);
		Py_END_ALLOW_THREADS

		if (!decompressor->ddict) {
			PyErr_SetString(ZstdError, "could not create decompression dict");
			return NULL;
		}
	}

	/* If threadCount==1, we don't start a thread pool. But we do leverage the
	   same API for dispatching work. */
	workerStates = PyMem_Malloc(threadCount * sizeof(WorkerState));
	if (NULL == workerStates) {
		PyErr_NoMemory();
		goto finally;
	}

	memset(workerStates, 0, threadCount * sizeof(WorkerState));

	if (threadCount > 1) {
		pool = POOL_create(threadCount, 1);
		if (NULL == pool) {
			PyErr_SetString(ZstdError, "could not initialize zstd thread pool");
			goto finally;
		}
	}

	bytesPerWorker = frames->compressedSize / threadCount;

	for (i = 0; i < threadCount; i++) {
		workerStates[i].dctx = ZSTD_createDCtx();
		if (NULL == workerStates[i].dctx) {
			PyErr_NoMemory();
			goto finally;
		}

		ZSTD_copyDCtx(workerStates[i].dctx, decompressor->dctx);

		workerStates[i].ddict = decompressor->ddict;
		workerStates[i].framePointers = framePointers;
		workerStates[i].requireOutputSizes = 1;
	}

	Py_BEGIN_ALLOW_THREADS
	/* There are many ways to split work among workers.

	   For now, we take a simple approach of splitting work so each worker
	   gets roughly the same number of input bytes. This will result in more
	   starvation than running N>threadCount jobs. But it avoids complications
	   around state tracking, which could involve extra locking.
	*/
	for (i = 0; i < frames->framesSize; i++) {
		workerBytes += frames->frames[i].sourceSize;

		/*
		 * The last worker/thread needs to handle all remaining work. Don't
		 * trigger it prematurely. Defer to the block outside of the loop.
		 * (But still process this loop so workerBytes is correct.
		 */
		if (currentThread == threadCount - 1) {
			continue;
		}

		if (workerBytes >= bytesPerWorker) {
			workerStates[currentThread].startOffset = workerStartOffset;
			workerStates[currentThread].endOffset = i;
			workerStates[currentThread].totalSourceSize = workerBytes;

			if (threadCount > 1) {
				POOL_add(pool, (POOL_function)decompress_worker, &workerStates[currentThread]);
			}
			else {
				decompress_worker(&workerStates[currentThread]);
			}
			currentThread++;
			workerStartOffset = i + 1;
			workerBytes = 0;
		}
	}

	if (workerBytes) {
		workerStates[currentThread].startOffset = workerStartOffset;
		workerStates[currentThread].endOffset = frames->framesSize - 1;
		workerStates[currentThread].totalSourceSize = workerBytes;

		if (threadCount > 1) {
			POOL_add(pool, (POOL_function)decompress_worker, &workerStates[currentThread]);
		}
		else {
			decompress_worker(&workerStates[currentThread]);
		}
	}

	if (threadCount > 1) {
		POOL_free(pool);
		pool = NULL;
	}
	Py_END_ALLOW_THREADS

	for (i = 0; i < threadCount; i++) {
		switch (workerStates[i].error) {
		case WorkerError_none:
			break;

		case WorkerError_zstd:
			PyErr_Format(ZstdError, "error decompressing item %zd: %s",
				workerStates[i].errorOffset, ZSTD_getErrorName(workerStates[i].zresult));
			errored = 1;
			break;

		case WorkerError_memory:
			PyErr_NoMemory();
			errored = 1;
			break;

		case WorkerError_sizeMismatch:
			PyErr_Format(ZstdError, "error decompressing item %zd: decompressed %zu bytes; expected %llu",
				workerStates[i].errorOffset, workerStates[i].zresult,
				framePointers[workerStates[i].errorOffset].destSize);
			errored = 1;
			break;

		case WorkerError_unknownSize:
			PyErr_Format(PyExc_ValueError, "could not determine decompressed size of item %zd",
				workerStates[i].errorOffset);
			errored = 1;
			break;

		default:
			PyErr_Format(ZstdError, "unhandled error type: %d; this is a bug",
				workerStates[i].error);
			errored = 1;
			break;
		}

		if (errored) {
			break;
		}
	}

	if (errored) {
		goto finally;
	}

	segmentsCount = 0;
	for (i = 0; i < threadCount; i++) {
		segmentsCount += workerStates[i].destCount;
	}

	resultArg = PyTuple_New(segmentsCount);
	if (NULL == resultArg) {
		goto finally;
	}

	resultIndex = 0;

	for (i = 0; i < threadCount; i++) {
		Py_ssize_t bufferIndex;
		WorkerState* state = &workerStates[i];

		for (bufferIndex = 0; bufferIndex < state->destCount; bufferIndex++) {
			DestBuffer* destBuffer = &state->destBuffers[bufferIndex];

			bws = BufferWithSegments_FromMemory(destBuffer->dest, destBuffer->destSize,
				destBuffer->segments, destBuffer->segmentsSize);
			if (NULL == bws) {
				goto finally;
			}

			/*
			* Memory for buffer and segments was allocated using malloc() in worker
			* and the memory is transferred to the BufferWithSegments instance. So
			* tell instance to use free() and NULL the reference in the state struct
			* so it isn't freed below.
			*/
			bws->useFree = 1;
			destBuffer->dest = NULL;
			destBuffer->segments = NULL;

			PyTuple_SET_ITEM(resultArg, resultIndex++, (PyObject*)bws);
		}
	}

	result = (ZstdBufferWithSegmentsCollection*)PyObject_CallObject(
		(PyObject*)&ZstdBufferWithSegmentsCollectionType, resultArg);

finally:
	Py_CLEAR(resultArg);

	if (workerStates) {
		for (i = 0; i < threadCount; i++) {
			Py_ssize_t bufferIndex;
			WorkerState* state = &workerStates[i];

			if (state->dctx) {
				ZSTD_freeDCtx(state->dctx);
			}

			for (bufferIndex = 0; bufferIndex < state->destCount; bufferIndex++) {
				if (state->destBuffers) {
					/*
					* Will be NULL if memory transfered to a BufferWithSegments.
					* Otherwise it is left over after an error occurred.
					*/
					free(state->destBuffers[bufferIndex].dest);
					free(state->destBuffers[bufferIndex].segments);
				}
			}

			free(state->destBuffers);
		}

		PyMem_Free(workerStates);
	}

	POOL_free(pool);

	return result;
}

PyDoc_STRVAR(Decompressor_multi_decompress_to_buffer__doc__,
"Decompress multiple frames to output buffers\n"
"\n"
"Receives a ``BufferWithSegments``, a ``BufferWithSegmentsCollection`` or a\n"
"list of bytes-like objects. Each item in the passed collection should be a\n"
"compressed zstd frame.\n"
"\n"
"Unless ``decompressed_sizes`` is specified, the content size *must* be\n"
"written into the zstd frame header. If ``decompressed_sizes`` is specified,\n"
"it is an object conforming to the buffer protocol that represents an array\n"
"of 64-bit unsigned integers in the machine's native format. Specifying\n"
"``decompressed_sizes`` avoids a pre-scan of each frame to determine its\n"
"output size.\n"
"\n"
"Returns a ``BufferWithSegmentsCollection`` containing the decompressed\n"
"data. All decompressed data is allocated in a single memory buffer. The\n"
"``BufferWithSegments`` instance tracks which objects are at which offsets\n"
"and their respective lengths.\n"
"\n"
"The ``threads`` argument controls how many threads to use for operations.\n"
"Negative values will use the same number of threads as logical CPUs on the\n"
"machine.\n"
);

static ZstdBufferWithSegmentsCollection* Decompressor_multi_decompress_to_buffer(ZstdDecompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"frames",
		"decompressed_sizes",
		"threads",
		NULL
	};

	PyObject* frames;
	Py_buffer frameSizes;
	int threads = 0;
	Py_ssize_t frameCount;
	Py_buffer* frameBuffers = NULL;
	FramePointer* framePointers = NULL;
	unsigned long long* frameSizesP = NULL;
	unsigned long long totalInputSize = 0;
	FrameSources frameSources;
	ZstdBufferWithSegmentsCollection* result = NULL;
	Py_ssize_t i;

	memset(&frameSizes, 0, sizeof(frameSizes));

#if PY_MAJOR_VERSION >= 3
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|y*i:multi_decompress_to_buffer",
#else
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|s*i:multi_decompress_to_buffer",
#endif
		kwlist, &frames, &frameSizes, &threads)) {
		return NULL;
	}

	if (frameSizes.buf) {
		if (!PyBuffer_IsContiguous(&frameSizes, 'C') || frameSizes.ndim > 1) {
			PyErr_SetString(PyExc_ValueError, "decompressed_sizes buffer should be contiguous and have a single dimension");
			goto finally;
		}

		frameSizesP = (unsigned long long*)frameSizes.buf;
	}

	if (threads < 0) {
		threads = cpu_count();
	}

	if (threads < 2) {
		threads = 1;
	}

	if (PyObject_TypeCheck(frames, &ZstdBufferWithSegmentsType)) {
		ZstdBufferWithSegments* buffer = (ZstdBufferWithSegments*)frames;
		frameCount = buffer->segmentCount;

		if (frameSizes.buf && frameSizes.len != frameCount * (Py_ssize_t)sizeof(unsigned long long)) {
			PyErr_Format(PyExc_ValueError, "decompressed_sizes size mismatch; expected %zd, got %zd",
				frameCount * sizeof(unsigned long long), frameSizes.len);
			goto finally;
		}

		framePointers = PyMem_Malloc(frameCount * sizeof(FramePointer));
		if (!framePointers) {
			PyErr_NoMemory();
			goto finally;
		}

		for (i = 0; i < frameCount; i++) {
			void* sourceData;
			unsigned long long sourceSize;
			unsigned long long decompressedSize = 0;

			if (buffer->segments[i].offset + buffer->segments[i].length > buffer->dataSize) {
				PyErr_Format(PyExc_ValueError, "item %zd has offset outside memory area", i);
				goto finally;
			}

			sourceData = (char*)buffer->data + buffer->segments[i].offset;
			sourceSize = buffer->segments[i].length;
			totalInputSize += sourceSize;

			if (frameSizesP) {
				decompressedSize = frameSizesP[i];
			}

			framePointers[i].sourceData = sourceData;
			framePointers[i].sourceSize = sourceSize;
			framePointers[i].destSize = decompressedSize;
		}
	}
	else if (PyObject_TypeCheck(frames, &ZstdBufferWithSegmentsCollectionType)) {
		Py_ssize_t offset = 0;
		ZstdBufferWithSegments* buffer;
		ZstdBufferWithSegmentsCollection* collection = (ZstdBufferWithSegmentsCollection*)frames;

		frameCount = BufferWithSegmentsCollection_length(collection);

		if (frameSizes.buf && frameSizes.len != frameCount) {
			PyErr_Format(PyExc_ValueError,
				"decompressed_sizes size mismatch; expected %zd; got %zd",
				frameCount * sizeof(unsigned long long), frameSizes.len);
			goto finally;
		}

		framePointers = PyMem_Malloc(frameCount * sizeof(FramePointer));
		if (NULL == framePointers) {
			PyErr_NoMemory();
			goto finally;
		}

		/* Iterate the data structure directly because it is faster. */
		for (i = 0; i < collection->bufferCount; i++) {
			Py_ssize_t segmentIndex;
			buffer = collection->buffers[i];

			for (segmentIndex = 0; segmentIndex < buffer->segmentCount; segmentIndex++) {
				if (buffer->segments[segmentIndex].offset + buffer->segments[segmentIndex].length > buffer->dataSize) {
					PyErr_Format(PyExc_ValueError, "item %zd has offset outside memory area",
						offset);
					goto finally;
				}

				totalInputSize += buffer->segments[segmentIndex].length;

				framePointers[offset].sourceData = (char*)buffer->data + buffer->segments[segmentIndex].offset;
				framePointers[offset].sourceSize = buffer->segments[segmentIndex].length;
				framePointers[offset].destSize = frameSizesP ? frameSizesP[offset] : 0;

				offset++;
			}
		}
	}
	else if (PyList_Check(frames)) {
		frameCount = PyList_GET_SIZE(frames);

		if (frameSizes.buf && frameSizes.len != frameCount * (Py_ssize_t)sizeof(unsigned long long)) {
			PyErr_Format(PyExc_ValueError, "decompressed_sizes size mismatch; expected %zd, got %zd",
				frameCount * sizeof(unsigned long long), frameSizes.len);
			goto finally;
		}

		framePointers = PyMem_Malloc(frameCount * sizeof(FramePointer));
		if (!framePointers) {
			PyErr_NoMemory();
			goto finally;
		}

		/*
		 * It is not clear whether Py_buffer.buf is still valid after
		 * PyBuffer_Release. So, we hold a reference to all Py_buffer instances
		 * for the duration of the operation.
		 */
		frameBuffers = PyMem_Malloc(frameCount * sizeof(Py_buffer));
		if (NULL == frameBuffers) {
			PyErr_NoMemory();
			goto finally;
		}

		memset(frameBuffers, 0, frameCount * sizeof(Py_buffer));

		/* Do a pass to assemble info about our input buffers and output sizes. */
		for (i = 0; i < frameCount; i++) {
			if (0 != PyObject_GetBuffer(PyList_GET_ITEM(frames, i),
				&frameBuffers[i], PyBUF_CONTIG_RO)) {
				PyErr_Clear();
				PyErr_Format(PyExc_TypeError, "item %zd not a bytes like object", i);
				goto finally;
			}

			totalInputSize += frameBuffers[i].len;

			framePointers[i].sourceData = frameBuffers[i].buf;
			framePointers[i].sourceSize = frameBuffers[i].len;
			framePointers[i].destSize = frameSizesP ? frameSizesP[i] : 0;
		}
	}
	else {
		PyErr_SetString(PyExc_TypeError, "argument must be list or BufferWithSegments");
		goto finally;
	}

	/* We now have an array with info about our inputs and outputs. Feed it into
	   our generic decompression function. */
	frameSources.frames = framePointers;
	frameSources.framesSize = frameCount;
	frameSources.compressedSize = totalInputSize;

	result = decompress_from_framesources(self, &frameSources, threads);

finally:
	if (frameSizes.buf) {
		PyBuffer_Release(&frameSizes);
	}
	PyMem_Free(framePointers);

	if (frameBuffers) {
		for (i = 0; i < frameCount; i++) {
			PyBuffer_Release(&frameBuffers[i]);
		}

		PyMem_Free(frameBuffers);
	}

	return result;
}

static PyMethodDef Decompressor_methods[] = {
	{ "copy_stream", (PyCFunction)Decompressor_copy_stream, METH_VARARGS | METH_KEYWORDS,
	Decompressor_copy_stream__doc__ },
	{ "decompress", (PyCFunction)Decompressor_decompress, METH_VARARGS | METH_KEYWORDS,
	Decompressor_decompress__doc__ },
	{ "decompressobj", (PyCFunction)Decompressor_decompressobj, METH_NOARGS,
	Decompressor_decompressobj__doc__ },
	{ "read_from", (PyCFunction)Decompressor_read_from, METH_VARARGS | METH_KEYWORDS,
	Decompressor_read_from__doc__ },
	{ "write_to", (PyCFunction)Decompressor_write_to, METH_VARARGS | METH_KEYWORDS,
	Decompressor_write_to__doc__ },
	{ "decompress_content_dict_chain", (PyCFunction)Decompressor_decompress_content_dict_chain,
	  METH_VARARGS | METH_KEYWORDS, Decompressor_decompress_content_dict_chain__doc__ },
	{ "multi_decompress_to_buffer", (PyCFunction)Decompressor_multi_decompress_to_buffer,
	  METH_VARARGS | METH_KEYWORDS, Decompressor_multi_decompress_to_buffer__doc__ },
	{ NULL, NULL }
};

PyTypeObject ZstdDecompressorType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"zstd.ZstdDecompressor",        /* tp_name */
	sizeof(ZstdDecompressor),       /* tp_basicsize */
	0,                              /* tp_itemsize */
	(destructor)Decompressor_dealloc, /* tp_dealloc */
	0,                              /* tp_print */
	0,                              /* tp_getattr */
	0,                              /* tp_setattr */
	0,                              /* tp_compare */
	0,                              /* tp_repr */
	0,                              /* tp_as_number */
	0,                              /* tp_as_sequence */
	0,                              /* tp_as_mapping */
	0,                              /* tp_hash */
	0,                              /* tp_call */
	0,                              /* tp_str */
	0,                              /* tp_getattro */
	0,                              /* tp_setattro */
	0,                              /* tp_as_buffer */
	Py_TPFLAGS_DEFAULT | Py_TPFLAGS_BASETYPE, /* tp_flags */
	Decompressor__doc__,            /* tp_doc */
	0,                              /* tp_traverse */
	0,                              /* tp_clear */
	0,                              /* tp_richcompare */
	0,                              /* tp_weaklistoffset */
	0,                              /* tp_iter */
	0,                              /* tp_iternext */
	Decompressor_methods,           /* tp_methods */
	0,                              /* tp_members */
	0,                              /* tp_getset */
	0,                              /* tp_base */
	0,                              /* tp_dict */
	0,                              /* tp_descr_get */
	0,                              /* tp_descr_set */
	0,                              /* tp_dictoffset */
	(initproc)Decompressor_init,    /* tp_init */
	0,                              /* tp_alloc */
	PyType_GenericNew,              /* tp_new */
};

void decompressor_module_init(PyObject* mod) {
	Py_TYPE(&ZstdDecompressorType) = &PyType_Type;
	if (PyType_Ready(&ZstdDecompressorType) < 0) {
		return;
	}

	Py_INCREF((PyObject*)&ZstdDecompressorType);
	PyModule_AddObject(mod, "ZstdDecompressor",
		(PyObject*)&ZstdDecompressorType);
}
