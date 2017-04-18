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

int populate_cdict(ZstdCompressor* compressor, ZSTD_parameters* zparams) {
	ZSTD_customMem zmem;

	if (compressor->cdict || !compressor->dict || !compressor->dict->dictData) {
		return 0;
	}

	Py_BEGIN_ALLOW_THREADS
	memset(&zmem, 0, sizeof(zmem));
	compressor->cdict = ZSTD_createCDict_advanced(compressor->dict->dictData,
		compressor->dict->dictSize, 1, *zparams, zmem);
	Py_END_ALLOW_THREADS

	if (!compressor->cdict) {
		PyErr_SetString(ZstdError, "could not create compression dictionary");
		return 1;
	}

	return 0;
}

/**
 * Ensure the ZSTD_CStream on a ZstdCompressor instance is initialized.
 *
 * Returns 0 on success. Other value on failure. Will set a Python exception
 * on failure.
 */
int init_cstream(ZstdCompressor* compressor, unsigned long long sourceSize) {
	ZSTD_parameters zparams;
	void* dictData = NULL;
	size_t dictSize = 0;
	size_t zresult;

	if (compressor->cstream) {
		zresult = ZSTD_resetCStream(compressor->cstream, sourceSize);
		if (ZSTD_isError(zresult)) {
			PyErr_Format(ZstdError, "could not reset CStream: %s",
				ZSTD_getErrorName(zresult));
			return -1;
		}

		return 0;
	}

	compressor->cstream = ZSTD_createCStream();
	if (!compressor->cstream) {
		PyErr_SetString(ZstdError, "could not create CStream");
		return -1;
	}

	if (compressor->dict) {
		dictData = compressor->dict->dictData;
		dictSize = compressor->dict->dictSize;
	}

	memset(&zparams, 0, sizeof(zparams));
	if (compressor->cparams) {
		ztopy_compression_parameters(compressor->cparams, &zparams.cParams);
		/* Do NOT call ZSTD_adjustCParams() here because the compression params
		come from the user. */
	}
	else {
		zparams.cParams = ZSTD_getCParams(compressor->compressionLevel, sourceSize, dictSize);
	}

	zparams.fParams = compressor->fparams;

	zresult = ZSTD_initCStream_advanced(compressor->cstream, dictData, dictSize,
		zparams, sourceSize);

	if (ZSTD_isError(zresult)) {
		ZSTD_freeCStream(compressor->cstream);
		compressor->cstream = NULL;
		PyErr_Format(ZstdError, "cannot init CStream: %s", ZSTD_getErrorName(zresult));
		return -1;
	}

	return 0;;
}

int init_mtcstream(ZstdCompressor* compressor, Py_ssize_t sourceSize) {
	size_t zresult;
	void* dictData = NULL;
	size_t dictSize = 0;
	ZSTD_parameters zparams;

	assert(compressor->mtcctx);

	if (compressor->dict) {
		dictData = compressor->dict->dictData;
		dictSize = compressor->dict->dictSize;
	}

	memset(&zparams, 0, sizeof(zparams));
	if (compressor->cparams) {
		ztopy_compression_parameters(compressor->cparams, &zparams.cParams);
	}
	else {
		zparams.cParams = ZSTD_getCParams(compressor->compressionLevel, sourceSize, dictSize);
	}

	zparams.fParams = compressor->fparams;

	zresult = ZSTDMT_initCStream_advanced(compressor->mtcctx, dictData, dictSize,
		zparams, sourceSize);

	if (ZSTD_isError(zresult)) {
		PyErr_Format(ZstdError, "cannot init CStream: %s", ZSTD_getErrorName(zresult));
		return -1;
	}

	return 0;
}

PyDoc_STRVAR(ZstdCompressor__doc__,
"ZstdCompressor(level=None, dict_data=None, compression_params=None)\n"
"\n"
"Create an object used to perform Zstandard compression.\n"
"\n"
"An instance can compress data various ways. Instances can be used multiple\n"
"times. Each compression operation will use the compression parameters\n"
"defined at construction time.\n"
"\n"
"Compression can be configured via the following names arguments:\n"
"\n"
"level\n"
"   Integer compression level.\n"
"dict_data\n"
"   A ``ZstdCompressionDict`` to be used to compress with dictionary data.\n"
"compression_params\n"
"   A ``CompressionParameters`` instance defining low-level compression"
"   parameters. If defined, this will overwrite the ``level`` argument.\n"
"write_checksum\n"
"   If True, a 4 byte content checksum will be written with the compressed\n"
"   data, allowing the decompressor to perform content verification.\n"
"write_content_size\n"
"   If True, the decompressed content size will be included in the header of\n"
"   the compressed data. This data will only be written if the compressor\n"
"   knows the size of the input data.\n"
"write_dict_id\n"
"   Determines whether the dictionary ID will be written into the compressed\n"
"   data. Defaults to True. Only adds content to the compressed data if\n"
"   a dictionary is being used.\n"
"threads\n"
"   Number of threads to use to compress data concurrently. When set,\n"
"   compression operations are performed on multiple threads. The default\n"
"   value (0) disables multi-threaded compression. A value of ``-1`` means to\n"
"   set the number of threads to the number of detected logical CPUs.\n"
);

static int ZstdCompressor_init(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"level",
		"dict_data",
		"compression_params",
		"write_checksum",
		"write_content_size",
		"write_dict_id",
		"threads",
		NULL
	};

	int level = 3;
	ZstdCompressionDict* dict = NULL;
	CompressionParametersObject* params = NULL;
	PyObject* writeChecksum = NULL;
	PyObject* writeContentSize = NULL;
	PyObject* writeDictID = NULL;
	int threads = 0;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "|iO!O!OOOi:ZstdCompressor",
		kwlist,	&level, &ZstdCompressionDictType, &dict,
		&CompressionParametersType, &params,
		&writeChecksum, &writeContentSize, &writeDictID, &threads)) {
		return -1;
	}

	if (level < 1) {
		PyErr_SetString(PyExc_ValueError, "level must be greater than 0");
		return -1;
	}

	if (level > ZSTD_maxCLevel()) {
		PyErr_Format(PyExc_ValueError, "level must be less than %d",
			ZSTD_maxCLevel() + 1);
		return -1;
	}

	if (threads < 0) {
		threads = cpu_count();
	}

	self->threads = threads;

	/* We create a ZSTD_CCtx for reuse among multiple operations to reduce the
	   overhead of each compression operation. */
	if (threads) {
		self->mtcctx = ZSTDMT_createCCtx(threads);
		if (!self->mtcctx) {
			PyErr_NoMemory();
			return -1;
		}
	}
	else {
		self->cctx = ZSTD_createCCtx();
		if (!self->cctx) {
			PyErr_NoMemory();
			return -1;
		}
	}

	self->compressionLevel = level;

	if (dict) {
		self->dict = dict;
		Py_INCREF(dict);
	}

	if (params) {
		self->cparams = params;
		Py_INCREF(params);
	}

	memset(&self->fparams, 0, sizeof(self->fparams));

	if (writeChecksum && PyObject_IsTrue(writeChecksum)) {
		self->fparams.checksumFlag = 1;
	}
	if (writeContentSize && PyObject_IsTrue(writeContentSize)) {
		self->fparams.contentSizeFlag = 1;
	}
	if (writeDictID && PyObject_Not(writeDictID)) {
		self->fparams.noDictIDFlag = 1;
	}

	return 0;
}

static void ZstdCompressor_dealloc(ZstdCompressor* self) {
	if (self->cstream) {
		ZSTD_freeCStream(self->cstream);
		self->cstream = NULL;
	}

	Py_XDECREF(self->cparams);
	Py_XDECREF(self->dict);

	if (self->cdict) {
		ZSTD_freeCDict(self->cdict);
		self->cdict = NULL;
	}

	if (self->cctx) {
		ZSTD_freeCCtx(self->cctx);
		self->cctx = NULL;
	}

	if (self->mtcctx) {
		ZSTDMT_freeCCtx(self->mtcctx);
		self->mtcctx = NULL;
	}

	PyObject_Del(self);
}

PyDoc_STRVAR(ZstdCompressor_copy_stream__doc__,
"copy_stream(ifh, ofh[, size=0, read_size=default, write_size=default])\n"
"compress data between streams\n"
"\n"
"Data will be read from ``ifh``, compressed, and written to ``ofh``.\n"
"``ifh`` must have a ``read(size)`` method. ``ofh`` must have a ``write(data)``\n"
"method.\n"
"\n"
"An optional ``size`` argument specifies the size of the source stream.\n"
"If defined, compression parameters will be tuned based on the size.\n"
"\n"
"Optional arguments ``read_size`` and ``write_size`` define the chunk sizes\n"
"of ``read()`` and ``write()`` operations, respectively. By default, they use\n"
"the default compression stream input and output sizes, respectively.\n"
);

static PyObject* ZstdCompressor_copy_stream(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"ifh",
		"ofh",
		"size",
		"read_size",
		"write_size",
		NULL
	};

	PyObject* source;
	PyObject* dest;
	Py_ssize_t sourceSize = 0;
	size_t inSize = ZSTD_CStreamInSize();
	size_t outSize = ZSTD_CStreamOutSize();
	ZSTD_inBuffer input;
	ZSTD_outBuffer output;
	Py_ssize_t totalRead = 0;
	Py_ssize_t totalWrite = 0;
	char* readBuffer;
	Py_ssize_t readSize;
	PyObject* readResult;
	PyObject* res = NULL;
	size_t zresult;
	PyObject* writeResult;
	PyObject* totalReadPy;
	PyObject* totalWritePy;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "OO|nkk:copy_stream", kwlist,
		&source, &dest, &sourceSize, &inSize, &outSize)) {
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

	if (self->mtcctx) {
		if (init_mtcstream(self, sourceSize)) {
			res = NULL;
			goto finally;
		}
	}
	else {
		if (0 != init_cstream(self, sourceSize)) {
			res = NULL;
			goto finally;
		}
	}

	output.dst = PyMem_Malloc(outSize);
	if (!output.dst) {
		PyErr_NoMemory();
		res = NULL;
		goto finally;
	}
	output.size = outSize;
	output.pos = 0;

	while (1) {
		/* Try to read from source stream. */
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

		/* Send data to compressor */
		input.src = readBuffer;
		input.size = readSize;
		input.pos = 0;

		while (input.pos < input.size) {
			Py_BEGIN_ALLOW_THREADS
			if (self->mtcctx) {
				zresult = ZSTDMT_compressStream(self->mtcctx, &output, &input);
			}
			else {
				zresult = ZSTD_compressStream(self->cstream, &output, &input);
			}
			Py_END_ALLOW_THREADS

			if (ZSTD_isError(zresult)) {
				res = NULL;
				PyErr_Format(ZstdError, "zstd compress error: %s", ZSTD_getErrorName(zresult));
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

	/* We've finished reading. Now flush the compressor stream. */
	while (1) {
		if (self->mtcctx) {
			zresult = ZSTDMT_endStream(self->mtcctx, &output);
		}
		else {
			zresult = ZSTD_endStream(self->cstream, &output);
		}
		if (ZSTD_isError(zresult)) {
			PyErr_Format(ZstdError, "error ending compression stream: %s",
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
			totalWrite += output.pos;
			Py_XDECREF(writeResult);
			output.pos = 0;
		}

		if (!zresult) {
			break;
		}
	}

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

PyDoc_STRVAR(ZstdCompressor_compress__doc__,
"compress(data, allow_empty=False)\n"
"\n"
"Compress data in a single operation.\n"
"\n"
"This is the simplest mechanism to perform compression: simply pass in a\n"
"value and get a compressed value back. It is almost the most prone to abuse.\n"
"The input and output values must fit in memory, so passing in very large\n"
"values can result in excessive memory usage. For this reason, one of the\n"
"streaming based APIs is preferred for larger values.\n"
);

static PyObject* ZstdCompressor_compress(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"data",
		"allow_empty",
		NULL
	};

	const char* source;
	Py_ssize_t sourceSize;
	PyObject* allowEmpty = NULL;
	size_t destSize;
	PyObject* output;
	char* dest;
	void* dictData = NULL;
	size_t dictSize = 0;
	size_t zresult;
	ZSTD_parameters zparams;

#if PY_MAJOR_VERSION >= 3
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "y#|O:compress",
#else
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "s#|O:compress",
#endif
		kwlist, &source, &sourceSize, &allowEmpty)) {
		return NULL;
	}

	if (self->threads && self->dict) {
		PyErr_SetString(ZstdError,
			"compress() cannot be used with both dictionaries and multi-threaded compression");
		return NULL;
	}

	if (self->threads && self->cparams) {
		PyErr_SetString(ZstdError,
			"compress() cannot be used with both compression parameters and multi-threaded compression");
		return NULL;
	}

	/* Limitation in zstd C API doesn't let decompression side distinguish
	   between content size of 0 and unknown content size. This can make round
	   tripping via Python difficult. Until this is fixed, require a flag
	   to fire the footgun.
	   https://github.com/indygreg/python-zstandard/issues/11 */
	if (0 == sourceSize && self->fparams.contentSizeFlag
		&& (!allowEmpty || PyObject_Not(allowEmpty))) {
		PyErr_SetString(PyExc_ValueError, "cannot write empty inputs when writing content sizes");
		return NULL;
	}

	destSize = ZSTD_compressBound(sourceSize);
	output = PyBytes_FromStringAndSize(NULL, destSize);
	if (!output) {
		return NULL;
	}

	dest = PyBytes_AsString(output);

	if (self->dict) {
		dictData = self->dict->dictData;
		dictSize = self->dict->dictSize;
	}

	memset(&zparams, 0, sizeof(zparams));
	if (!self->cparams) {
		zparams.cParams = ZSTD_getCParams(self->compressionLevel, sourceSize, dictSize);
	}
	else {
		ztopy_compression_parameters(self->cparams, &zparams.cParams);
		/* Do NOT call ZSTD_adjustCParams() here because the compression params
		come from the user. */
	}

	zparams.fParams = self->fparams;

	/* The raw dict data has to be processed before it can be used. Since this
	adds overhead - especially if multiple dictionary compression operations
	are performed on the same ZstdCompressor instance - we create a
	ZSTD_CDict once and reuse it for all operations.

	Note: the compression parameters used for the first invocation (possibly
	derived from the source size) will be reused on all subsequent invocations.
	https://github.com/facebook/zstd/issues/358 contains more info. We could
	potentially add an argument somewhere to control this behavior.
	*/
	if (0 != populate_cdict(self, &zparams)) {
		Py_DECREF(output);
		return NULL;
	}

	Py_BEGIN_ALLOW_THREADS
	if (self->mtcctx) {
		zresult = ZSTDMT_compressCCtx(self->mtcctx, dest, destSize,
			source, sourceSize, self->compressionLevel);
	}
	else {
		/* By avoiding ZSTD_compress(), we don't necessarily write out content
		   size. This means the argument to ZstdCompressor to control frame
		   parameters is honored. */
		if (self->cdict) {
			zresult = ZSTD_compress_usingCDict(self->cctx, dest, destSize,
				source, sourceSize, self->cdict);
		}
		else {
			zresult = ZSTD_compress_advanced(self->cctx, dest, destSize,
				source, sourceSize, dictData, dictSize, zparams);
		}
	}
	Py_END_ALLOW_THREADS

	if (ZSTD_isError(zresult)) {
		PyErr_Format(ZstdError, "cannot compress: %s", ZSTD_getErrorName(zresult));
		Py_CLEAR(output);
		return NULL;
	}
	else {
		Py_SIZE(output) = zresult;
	}

	return output;
}

PyDoc_STRVAR(ZstdCompressionObj__doc__,
"compressobj()\n"
"\n"
"Return an object exposing ``compress(data)`` and ``flush()`` methods.\n"
"\n"
"The returned object exposes an API similar to ``zlib.compressobj`` and\n"
"``bz2.BZ2Compressor`` so that callers can swap in the zstd compressor\n"
"without changing how compression is performed.\n"
);

static ZstdCompressionObj* ZstdCompressor_compressobj(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"size",
		NULL
	};

	Py_ssize_t inSize = 0;
	size_t outSize = ZSTD_CStreamOutSize();
	ZstdCompressionObj* result = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "|n:compressobj", kwlist, &inSize)) {
		return NULL;
	}

	result = (ZstdCompressionObj*)PyObject_CallObject((PyObject*)&ZstdCompressionObjType, NULL);
	if (!result) {
		return NULL;
	}

	if (self->mtcctx) {
		if (init_mtcstream(self, inSize)) {
			Py_DECREF(result);
			return NULL;
		}
	}
	else {
		if (0 != init_cstream(self, inSize)) {
			Py_DECREF(result);
			return NULL;
		}
	}

	result->output.dst = PyMem_Malloc(outSize);
	if (!result->output.dst) {
		PyErr_NoMemory();
		Py_DECREF(result);
		return NULL;
	}
	result->output.size = outSize;
	result->compressor = self;
	Py_INCREF(result->compressor);

	return result;
}

PyDoc_STRVAR(ZstdCompressor_read_from__doc__,
"read_from(reader, [size=0, read_size=default, write_size=default])\n"
"Read uncompress data from a reader and return an iterator\n"
"\n"
"Returns an iterator of compressed data produced from reading from ``reader``.\n"
"\n"
"Uncompressed data will be obtained from ``reader`` by calling the\n"
"``read(size)`` method of it. The source data will be streamed into a\n"
"compressor. As compressed data is available, it will be exposed to the\n"
"iterator.\n"
"\n"
"Data is read from the source in chunks of ``read_size``. Compressed chunks\n"
"are at most ``write_size`` bytes. Both values default to the zstd input and\n"
"and output defaults, respectively.\n"
"\n"
"The caller is partially in control of how fast data is fed into the\n"
"compressor by how it consumes the returned iterator. The compressor will\n"
"not consume from the reader unless the caller consumes from the iterator.\n"
);

static ZstdCompressorIterator* ZstdCompressor_read_from(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"reader",
		"size",
		"read_size",
		"write_size",
		NULL
	};

	PyObject* reader;
	Py_ssize_t sourceSize = 0;
	size_t inSize = ZSTD_CStreamInSize();
	size_t outSize = ZSTD_CStreamOutSize();
	ZstdCompressorIterator* result;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|nkk:read_from", kwlist,
		&reader, &sourceSize, &inSize, &outSize)) {
		return NULL;
	}

	result = (ZstdCompressorIterator*)PyObject_CallObject((PyObject*)&ZstdCompressorIteratorType, NULL);
	if (!result) {
		return NULL;
	}
	if (PyObject_HasAttrString(reader, "read")) {
		result->reader = reader;
		Py_INCREF(result->reader);
	}
	else if (1 == PyObject_CheckBuffer(reader)) {
		result->buffer = PyMem_Malloc(sizeof(Py_buffer));
		if (!result->buffer) {
			goto except;
		}

		memset(result->buffer, 0, sizeof(Py_buffer));

		if (0 != PyObject_GetBuffer(reader, result->buffer, PyBUF_CONTIG_RO)) {
			goto except;
		}

		sourceSize = result->buffer->len;
	}
	else {
		PyErr_SetString(PyExc_ValueError,
			"must pass an object with a read() method or conforms to buffer protocol");
		goto except;
	}

	result->compressor = self;
	Py_INCREF(result->compressor);

	result->sourceSize = sourceSize;

	if (self->mtcctx) {
		if (init_mtcstream(self, sourceSize)) {
			goto except;
		}
	}
	else {
		if (0 != init_cstream(self, sourceSize)) {
			goto except;
		}
	}

	result->inSize = inSize;
	result->outSize = outSize;

	result->output.dst = PyMem_Malloc(outSize);
	if (!result->output.dst) {
		PyErr_NoMemory();
		goto except;
	}
	result->output.size = outSize;

	goto finally;

except:
	Py_XDECREF(result->compressor);
	Py_XDECREF(result->reader);
	Py_DECREF(result);
	result = NULL;

finally:
	return result;
}

PyDoc_STRVAR(ZstdCompressor_write_to___doc__,
"Create a context manager to write compressed data to an object.\n"
"\n"
"The passed object must have a ``write()`` method.\n"
"\n"
"The caller feeds input data to the object by calling ``compress(data)``.\n"
"Compressed data is written to the argument given to this function.\n"
"\n"
"The function takes an optional ``size`` argument indicating the total size\n"
"of the eventual input. If specified, the size will influence compression\n"
"parameter tuning and could result in the size being written into the\n"
"header of the compressed data.\n"
"\n"
"An optional ``write_size`` argument is also accepted. It defines the maximum\n"
"byte size of chunks fed to ``write()``. By default, it uses the zstd default\n"
"for a compressor output stream.\n"
);

static ZstdCompressionWriter* ZstdCompressor_write_to(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"writer",
		"size",
		"write_size",
		NULL
	};

	PyObject* writer;
	ZstdCompressionWriter* result;
	Py_ssize_t sourceSize = 0;
	size_t outSize = ZSTD_CStreamOutSize();

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|nk:write_to", kwlist,
		&writer, &sourceSize, &outSize)) {
		return NULL;
	}

	if (!PyObject_HasAttrString(writer, "write")) {
		PyErr_SetString(PyExc_ValueError, "must pass an object with a write() method");
		return NULL;
	}

	result = (ZstdCompressionWriter*)PyObject_CallObject((PyObject*)&ZstdCompressionWriterType, NULL);
	if (!result) {
		return NULL;
	}

	result->compressor = self;
	Py_INCREF(result->compressor);

	result->writer = writer;
	Py_INCREF(result->writer);

	result->sourceSize = sourceSize;
	result->outSize = outSize;

	return result;
}

typedef struct {
	void* sourceData;
	size_t sourceSize;
} DataSource;

typedef struct {
	DataSource* sources;
	Py_ssize_t sourcesSize;
	unsigned long long totalSourceSize;
} DataSources;

typedef struct {
	void* dest;
	Py_ssize_t destSize;
	BufferSegment* segments;
	Py_ssize_t segmentsSize;
} DestBuffer;

typedef enum {
	WorkerError_none = 0,
	WorkerError_zstd = 1,
	WorkerError_no_memory = 2,
} WorkerError;

/**
 * Holds state for an individual worker performing multi_compress_to_buffer work.
 */
typedef struct {
	/* Used for compression. */
	ZSTD_CCtx* cctx;
	ZSTD_CDict* cdict;
	int cLevel;
	CompressionParametersObject* cParams;
	ZSTD_frameParameters fParams;

	/* What to compress. */
	DataSource* sources;
	Py_ssize_t sourcesSize;
	Py_ssize_t startOffset;
	Py_ssize_t endOffset;
	unsigned long long totalSourceSize;

	/* Result storage. */
	DestBuffer* destBuffers;
	Py_ssize_t destCount;

	/* Error tracking. */
	WorkerError error;
	size_t zresult;
	Py_ssize_t errorOffset;
} WorkerState;

static void compress_worker(WorkerState* state) {
	Py_ssize_t inputOffset = state->startOffset;
	Py_ssize_t remainingItems = state->endOffset - state->startOffset + 1;
	Py_ssize_t currentBufferStartOffset = state->startOffset;
	size_t zresult;
	ZSTD_parameters zparams;
	void* newDest;
	size_t allocationSize;
	size_t boundSize;
	Py_ssize_t destOffset = 0;
	DataSource* sources = state->sources;
	DestBuffer* destBuffer;

	assert(!state->destBuffers);
	assert(0 == state->destCount);

	if (state->cParams) {
		ztopy_compression_parameters(state->cParams, &zparams.cParams);
	}

	zparams.fParams = state->fParams;

	/*
	 * The total size of the compressed data is unknown until we actually
	 * compress data. That means we can't pre-allocate the exact size we need.
	 * 
	 * There is a cost to every allocation and reallocation. So, it is in our
	 * interest to minimize the number of allocations.
	 *
	 * There is also a cost to too few allocations. If allocations are too
	 * large they may fail. If buffers are shared and all inputs become
	 * irrelevant at different lifetimes, then a reference to one segment
	 * in the buffer will keep the entire buffer alive. This leads to excessive
	 * memory usage.
	 *
	 * Our current strategy is to assume a compression ratio of 16:1 and
	 * allocate buffers of that size, rounded up to the nearest power of 2
	 * (because computers like round numbers). That ratio is greater than what
	 * most inputs achieve. This is by design: we don't want to over-allocate.
	 * But we don't want to under-allocate and lead to too many buffers either.
	 */

	state->destCount = 1;

	state->destBuffers = calloc(1, sizeof(DestBuffer));
	if (NULL == state->destBuffers) {
		state->error = WorkerError_no_memory;
		return;
	}

	destBuffer = &state->destBuffers[state->destCount - 1];

	/*
	 * Rather than track bounds and grow the segments buffer, allocate space
	 * to hold remaining items then truncate when we're done with it.
	 */
	destBuffer->segments = calloc(remainingItems, sizeof(BufferSegment));
	if (NULL == destBuffer->segments) {
		state->error = WorkerError_no_memory;
		return;
	}

	destBuffer->segmentsSize = remainingItems;

	allocationSize = roundpow2(state->totalSourceSize >> 4);

	/* If the maximum size of the output is larger than that, round up. */
	boundSize = ZSTD_compressBound(sources[inputOffset].sourceSize);

	if (boundSize > allocationSize) {
		allocationSize = roundpow2(boundSize);
	}

	destBuffer->dest = malloc(allocationSize);
	if (NULL == destBuffer->dest) {
		state->error = WorkerError_no_memory;
		return;
	}

	destBuffer->destSize = allocationSize;

	for (inputOffset = state->startOffset; inputOffset <= state->endOffset; inputOffset++) {
		void* source = sources[inputOffset].sourceData;
		size_t sourceSize = sources[inputOffset].sourceSize;
		size_t destAvailable;
		void* dest;

		destAvailable = destBuffer->destSize - destOffset;
		boundSize = ZSTD_compressBound(sourceSize);

		/*
		 * Not enough space in current buffer to hold largest compressed output.
		 * So allocate and switch to a new output buffer.
		 */
		if (boundSize > destAvailable) {
			/*
			 * The downsizing of the existing buffer is optional. It should be cheap
			 * (unlike growing). So we just do it.
			 */
			if (destAvailable) {
				newDest = realloc(destBuffer->dest, destOffset);
				if (NULL == newDest) {
					state->error = WorkerError_no_memory;
					return;
				}

				destBuffer->dest = newDest;
				destBuffer->destSize = destOffset;
			}

			/* Truncate segments buffer. */
			newDest = realloc(destBuffer->segments,
				(inputOffset - currentBufferStartOffset + 1) * sizeof(BufferSegment));
			if (NULL == newDest) {
				state->error = WorkerError_no_memory;
				return;
			}

			destBuffer->segments = newDest;
			destBuffer->segmentsSize = inputOffset - currentBufferStartOffset;

			/* Grow space for new struct. */
			/* TODO consider over-allocating so we don't do this every time. */
			newDest = realloc(state->destBuffers, (state->destCount + 1) * sizeof(DestBuffer));
			if (NULL == newDest) {
				state->error = WorkerError_no_memory;
				return;
			}

			state->destBuffers = newDest;
			state->destCount++;

			destBuffer = &state->destBuffers[state->destCount - 1];

			/* Don't take any chances with non-NULL pointers. */
			memset(destBuffer, 0, sizeof(DestBuffer));

			/**
			 * We could dynamically update allocation size based on work done so far.
			 * For now, keep is simple.
			 */
			allocationSize = roundpow2(state->totalSourceSize >> 4);

			if (boundSize > allocationSize) {
				allocationSize = roundpow2(boundSize);
			}

			destBuffer->dest = malloc(allocationSize);
			if (NULL == destBuffer->dest) {
				state->error = WorkerError_no_memory;
				return;
			}

			destBuffer->destSize = allocationSize;
			destAvailable = allocationSize;
			destOffset = 0;

			destBuffer->segments = calloc(remainingItems, sizeof(BufferSegment));
			if (NULL == destBuffer->segments) {
				state->error = WorkerError_no_memory;
				return;
			}

			destBuffer->segmentsSize = remainingItems;
			currentBufferStartOffset = inputOffset;
		}

		dest = (char*)destBuffer->dest + destOffset;

		if (state->cdict) {
			zresult = ZSTD_compress_usingCDict(state->cctx, dest, destAvailable,
				source, sourceSize, state->cdict);
		}
		else {
			if (!state->cParams) {
				zparams.cParams = ZSTD_getCParams(state->cLevel, sourceSize, 0);
			}

			zresult = ZSTD_compress_advanced(state->cctx, dest, destAvailable,
				source, sourceSize, NULL, 0, zparams);
		}

		if (ZSTD_isError(zresult)) {
			state->error = WorkerError_zstd;
			state->zresult = zresult;
			state->errorOffset = inputOffset;
			break;
		}

		destBuffer->segments[inputOffset - currentBufferStartOffset].offset = destOffset;
		destBuffer->segments[inputOffset - currentBufferStartOffset].length = zresult;

		destOffset += zresult;
		remainingItems--;
	}

	if (destBuffer->destSize > destOffset) {
		newDest = realloc(destBuffer->dest, destOffset);
		if (NULL == newDest) {
			state->error = WorkerError_no_memory;
			return;
		}

		destBuffer->dest = newDest;
		destBuffer->destSize = destOffset;
	}
}

ZstdBufferWithSegmentsCollection* compress_from_datasources(ZstdCompressor* compressor,
	DataSources* sources, unsigned int threadCount) {
	ZSTD_parameters zparams;
	unsigned long long bytesPerWorker;
	POOL_ctx* pool = NULL;
	WorkerState* workerStates = NULL;
	Py_ssize_t i;
	unsigned long long workerBytes = 0;
	Py_ssize_t workerStartOffset = 0;
	size_t currentThread = 0;
	int errored = 0;
	Py_ssize_t segmentsCount = 0;
	Py_ssize_t segmentIndex;
	PyObject* segmentsArg = NULL;
	ZstdBufferWithSegments* buffer;
	ZstdBufferWithSegmentsCollection* result = NULL;

	assert(sources->sourcesSize > 0);
	assert(sources->totalSourceSize > 0);
	assert(threadCount >= 1);

	/* More threads than inputs makes no sense. */
	threadCount = sources->sourcesSize < threadCount ? (unsigned int)sources->sourcesSize
													 : threadCount;

	/* TODO lower thread count when input size is too small and threads would add
	overhead. */

	/*
	 * When dictionaries are used, parameters are derived from the size of the
	 * first element.
	 *
	 * TODO come up with a better mechanism.
	 */
	memset(&zparams, 0, sizeof(zparams));
	if (compressor->cparams) {
		ztopy_compression_parameters(compressor->cparams, &zparams.cParams);
	}
	else {
		zparams.cParams = ZSTD_getCParams(compressor->compressionLevel,
			sources->sources[0].sourceSize,
			compressor->dict ? compressor->dict->dictSize : 0);
	}

	zparams.fParams = compressor->fparams;

	if (0 != populate_cdict(compressor, &zparams)) {
		return NULL;
	}

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

	bytesPerWorker = sources->totalSourceSize / threadCount;

	for (i = 0; i < threadCount; i++) {
		workerStates[i].cctx = ZSTD_createCCtx();
		if (!workerStates[i].cctx) {
			PyErr_NoMemory();
			goto finally;
		}

		workerStates[i].cdict = compressor->cdict;
		workerStates[i].cLevel = compressor->compressionLevel;
		workerStates[i].cParams = compressor->cparams;
		workerStates[i].fParams = compressor->fparams;

		workerStates[i].sources = sources->sources;
		workerStates[i].sourcesSize = sources->sourcesSize;
	}

	Py_BEGIN_ALLOW_THREADS
	for (i = 0; i < sources->sourcesSize; i++) {
		workerBytes += sources->sources[i].sourceSize;

		/*
		 * The last worker/thread needs to handle all remaining work. Don't
		 * trigger it prematurely. Defer to the block outside of the loop
		 * to run the last worker/thread. But do still process this loop
		 * so workerBytes is correct.
		 */
		if (currentThread == threadCount - 1) {
			continue;
		}

		if (workerBytes >= bytesPerWorker) {
			assert(currentThread < threadCount);
			workerStates[currentThread].totalSourceSize = workerBytes;
			workerStates[currentThread].startOffset = workerStartOffset;
			workerStates[currentThread].endOffset = i;

			if (threadCount > 1) {
				POOL_add(pool, (POOL_function)compress_worker, &workerStates[currentThread]);
			}
			else {
				compress_worker(&workerStates[currentThread]);
			}

			currentThread++;
			workerStartOffset = i + 1;
			workerBytes = 0;
		}
	}

	if (workerBytes) {
		assert(currentThread < threadCount);
		workerStates[currentThread].totalSourceSize = workerBytes;
		workerStates[currentThread].startOffset = workerStartOffset;
		workerStates[currentThread].endOffset = sources->sourcesSize - 1;

		if (threadCount > 1) {
			POOL_add(pool, (POOL_function)compress_worker, &workerStates[currentThread]);
		}
		else {
			compress_worker(&workerStates[currentThread]);
		}
	}

	if (threadCount > 1) {
		POOL_free(pool);
		pool = NULL;
	}

	Py_END_ALLOW_THREADS

	for (i = 0; i < threadCount; i++) {
		switch (workerStates[i].error) {
		case WorkerError_no_memory:
			PyErr_NoMemory();
			errored = 1;
			break;

		case WorkerError_zstd:
			PyErr_Format(ZstdError, "error compressing item %zd: %s",
				workerStates[i].errorOffset, ZSTD_getErrorName(workerStates[i].zresult));
			errored = 1;
			break;
		default:
			;
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
		WorkerState* state = &workerStates[i];
		segmentsCount += state->destCount;
	}

	segmentsArg = PyTuple_New(segmentsCount);
	if (NULL == segmentsArg) {
		goto finally;
	}

	segmentIndex = 0;

	for (i = 0; i < threadCount; i++) {
		Py_ssize_t j;
		WorkerState* state = &workerStates[i];

		for (j = 0; j < state->destCount; j++) {
			DestBuffer* destBuffer = &state->destBuffers[j];
			buffer = BufferWithSegments_FromMemory(destBuffer->dest, destBuffer->destSize,
				destBuffer->segments, destBuffer->segmentsSize);

			if (NULL == buffer) {
				goto finally;
			}

			/* Tell instance to use free() instsead of PyMem_Free(). */
			buffer->useFree = 1;

			/*
			 * BufferWithSegments_FromMemory takes ownership of the backing memory.
			 * Unset it here so it doesn't get freed below.
			 */
			destBuffer->dest = NULL;
			destBuffer->segments = NULL;

			PyTuple_SET_ITEM(segmentsArg, segmentIndex++, (PyObject*)buffer);
		}
	}

	result = (ZstdBufferWithSegmentsCollection*)PyObject_CallObject(
		(PyObject*)&ZstdBufferWithSegmentsCollectionType, segmentsArg);

finally:
	Py_CLEAR(segmentsArg);

	if (pool) {
		POOL_free(pool);
	}

	if (workerStates) {
		Py_ssize_t j;

		for (i = 0; i < threadCount; i++) {
			WorkerState state = workerStates[i];

			if (state.cctx) {
				ZSTD_freeCCtx(state.cctx);
			}

			/* malloc() is used in worker thread. */

			for (j = 0; j < state.destCount; j++) {
				if (state.destBuffers) {
					free(state.destBuffers[j].dest);
					free(state.destBuffers[j].segments);
				}
			}


			free(state.destBuffers);
		}

		PyMem_Free(workerStates);
	}

	return result;
}

PyDoc_STRVAR(ZstdCompressor_multi_compress_to_buffer__doc__,
"Compress multiple pieces of data as a single operation\n"
"\n"
"Receives a ``BufferWithSegmentsCollection``, a ``BufferWithSegments``, or\n"
"a list of bytes like objects holding data to compress.\n"
"\n"
"Returns a ``BufferWithSegmentsCollection`` holding compressed data.\n"
"\n"
"This function is optimized to perform multiple compression operations as\n"
"as possible with as little overhead as possbile.\n"
);

static ZstdBufferWithSegmentsCollection* ZstdCompressor_multi_compress_to_buffer(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"data",
		"threads",
		NULL
	};

	PyObject* data;
	int threads = 0;
	Py_buffer* dataBuffers = NULL;
	DataSources sources;
	Py_ssize_t i;
	Py_ssize_t sourceCount = 0;
	ZstdBufferWithSegmentsCollection* result = NULL;

	if (self->mtcctx) {
		PyErr_SetString(ZstdError,
			"function cannot be called on ZstdCompressor configured for multi-threaded compression");
		return NULL;
	}

	memset(&sources, 0, sizeof(sources));

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|i:multi_compress_to_buffer", kwlist,
		&data, &threads)) {
		return NULL;
	}

	if (threads < 0) {
		threads = cpu_count();
	}

	if (threads < 2) {
		threads = 1;
	}

	if (PyObject_TypeCheck(data, &ZstdBufferWithSegmentsType)) {
		ZstdBufferWithSegments* buffer = (ZstdBufferWithSegments*)data;

		sources.sources = PyMem_Malloc(buffer->segmentCount * sizeof(DataSource));
		if (NULL == sources.sources) {
			PyErr_NoMemory();
			goto finally;
		}

		for (i = 0; i < buffer->segmentCount; i++) {
			sources.sources[i].sourceData = (char*)buffer->data + buffer->segments[i].offset;
			sources.sources[i].sourceSize = buffer->segments[i].length;
			sources.totalSourceSize += buffer->segments[i].length;
		}

		sources.sourcesSize = buffer->segmentCount;
	}
	else if (PyObject_TypeCheck(data, &ZstdBufferWithSegmentsCollectionType)) {
		Py_ssize_t j;
		Py_ssize_t offset = 0;
		ZstdBufferWithSegments* buffer;
		ZstdBufferWithSegmentsCollection* collection = (ZstdBufferWithSegmentsCollection*)data;

		sourceCount = BufferWithSegmentsCollection_length(collection);

		sources.sources = PyMem_Malloc(sourceCount * sizeof(DataSource));
		if (NULL == sources.sources) {
			PyErr_NoMemory();
			goto finally;
		}

		for (i = 0; i < collection->bufferCount; i++) {
			buffer = collection->buffers[i];

			for (j = 0; j < buffer->segmentCount; j++) {
				sources.sources[offset].sourceData = (char*)buffer->data + buffer->segments[j].offset;
				sources.sources[offset].sourceSize = buffer->segments[j].length;
				sources.totalSourceSize += buffer->segments[j].length;

				offset++;
			}
		}

		sources.sourcesSize = sourceCount;
	}
	else if (PyList_Check(data)) {
		sourceCount = PyList_GET_SIZE(data);

		sources.sources = PyMem_Malloc(sourceCount * sizeof(DataSource));
		if (NULL == sources.sources) {
			PyErr_NoMemory();
			goto finally;
		}

		/*
		 * It isn't clear whether the address referred to by Py_buffer.buf
		 * is still valid after PyBuffer_Release. We we hold a reference to all
		 * Py_buffer instances for the duration of the operation.
		 */
		dataBuffers = PyMem_Malloc(sourceCount * sizeof(Py_buffer));
		if (NULL == dataBuffers) {
			PyErr_NoMemory();
			goto finally;
		}

		memset(dataBuffers, 0, sourceCount * sizeof(Py_buffer));

		for (i = 0; i < sourceCount; i++) {
			if (0 != PyObject_GetBuffer(PyList_GET_ITEM(data, i),
				&dataBuffers[i], PyBUF_CONTIG_RO)) {
				PyErr_Clear();
				PyErr_Format(PyExc_TypeError, "item %zd not a bytes like object", i);
				goto finally;
			}

			sources.sources[i].sourceData = dataBuffers[i].buf;
			sources.sources[i].sourceSize = dataBuffers[i].len;
			sources.totalSourceSize += dataBuffers[i].len;
		}

		sources.sourcesSize = sourceCount;
	}
	else {
		PyErr_SetString(PyExc_TypeError, "argument must be list of BufferWithSegments");
		goto finally;
	}

	if (0 == sources.sourcesSize) {
		PyErr_SetString(PyExc_ValueError, "no source elements found");
		goto finally;
	}

	if (0 == sources.totalSourceSize) {
		PyErr_SetString(PyExc_ValueError, "source elements are empty");
		goto finally;
	}

	result = compress_from_datasources(self, &sources, threads);

finally:
	PyMem_Free(sources.sources);

	if (dataBuffers) {
		for (i = 0; i < sourceCount; i++) {
			PyBuffer_Release(&dataBuffers[i]);
		}

		PyMem_Free(dataBuffers);
	}

	return result;
}

static PyMethodDef ZstdCompressor_methods[] = {
	{ "compress", (PyCFunction)ZstdCompressor_compress,
	METH_VARARGS | METH_KEYWORDS, ZstdCompressor_compress__doc__ },
	{ "compressobj", (PyCFunction)ZstdCompressor_compressobj,
	METH_VARARGS | METH_KEYWORDS, ZstdCompressionObj__doc__ },
	{ "copy_stream", (PyCFunction)ZstdCompressor_copy_stream,
	METH_VARARGS | METH_KEYWORDS, ZstdCompressor_copy_stream__doc__ },
	{ "read_from", (PyCFunction)ZstdCompressor_read_from,
	METH_VARARGS | METH_KEYWORDS, ZstdCompressor_read_from__doc__ },
	{ "write_to", (PyCFunction)ZstdCompressor_write_to,
	METH_VARARGS | METH_KEYWORDS, ZstdCompressor_write_to___doc__ },
	{ "multi_compress_to_buffer", (PyCFunction)ZstdCompressor_multi_compress_to_buffer,
	METH_VARARGS | METH_KEYWORDS, ZstdCompressor_multi_compress_to_buffer__doc__ },
	{ NULL, NULL }
};

PyTypeObject ZstdCompressorType = {
	PyVarObject_HEAD_INIT(NULL, 0)
	"zstd.ZstdCompressor",         /* tp_name */
	sizeof(ZstdCompressor),        /* tp_basicsize */
	0,                              /* tp_itemsize */
	(destructor)ZstdCompressor_dealloc, /* tp_dealloc */
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
	ZstdCompressor__doc__,          /* tp_doc */
	0,                              /* tp_traverse */
	0,                              /* tp_clear */
	0,                              /* tp_richcompare */
	0,                              /* tp_weaklistoffset */
	0,                              /* tp_iter */
	0,                              /* tp_iternext */
	ZstdCompressor_methods,         /* tp_methods */
	0,                              /* tp_members */
	0,                              /* tp_getset */
	0,                              /* tp_base */
	0,                              /* tp_dict */
	0,                              /* tp_descr_get */
	0,                              /* tp_descr_set */
	0,                              /* tp_dictoffset */
	(initproc)ZstdCompressor_init,  /* tp_init */
	0,                              /* tp_alloc */
	PyType_GenericNew,              /* tp_new */
};

void compressor_module_init(PyObject* mod) {
	Py_TYPE(&ZstdCompressorType) = &PyType_Type;
	if (PyType_Ready(&ZstdCompressorType) < 0) {
		return;
	}

	Py_INCREF((PyObject*)&ZstdCompressorType);
	PyModule_AddObject(mod, "ZstdCompressor",
		(PyObject*)&ZstdCompressorType);
}
