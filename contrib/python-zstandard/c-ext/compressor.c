/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"

extern PyObject* ZstdError;

int populate_cdict(ZstdCompressor* compressor, void* dictData, size_t dictSize, ZSTD_parameters* zparams) {
	ZSTD_customMem zmem;
	assert(!compressor->cdict);
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
* Initialize a zstd CStream from a ZstdCompressor instance.
*
* Returns a ZSTD_CStream on success or NULL on failure. If NULL, a Python
* exception will be set.
*/
ZSTD_CStream* CStream_from_ZstdCompressor(ZstdCompressor* compressor, Py_ssize_t sourceSize) {
	ZSTD_CStream* cstream;
	ZSTD_parameters zparams;
	void* dictData = NULL;
	size_t dictSize = 0;
	size_t zresult;

	cstream = ZSTD_createCStream();
	if (!cstream) {
		PyErr_SetString(ZstdError, "cannot create CStream");
		return NULL;
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

	zresult = ZSTD_initCStream_advanced(cstream, dictData, dictSize, zparams, sourceSize);

	if (ZSTD_isError(zresult)) {
		ZSTD_freeCStream(cstream);
		PyErr_Format(ZstdError, "cannot init CStream: %s", ZSTD_getErrorName(zresult));
		return NULL;
	}

	return cstream;
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
);

static int ZstdCompressor_init(ZstdCompressor* self, PyObject* args, PyObject* kwargs) {
	static char* kwlist[] = {
		"level",
		"dict_data",
		"compression_params",
		"write_checksum",
		"write_content_size",
		"write_dict_id",
		NULL
	};

	int level = 3;
	ZstdCompressionDict* dict = NULL;
	CompressionParametersObject* params = NULL;
	PyObject* writeChecksum = NULL;
	PyObject* writeContentSize = NULL;
	PyObject* writeDictID = NULL;

	self->cctx = NULL;
	self->dict = NULL;
	self->cparams = NULL;
	self->cdict = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "|iO!O!OOO:ZstdCompressor",
		kwlist,	&level, &ZstdCompressionDictType, &dict,
		&CompressionParametersType, &params,
		&writeChecksum, &writeContentSize, &writeDictID)) {
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

	/* We create a ZSTD_CCtx for reuse among multiple operations to reduce the
	   overhead of each compression operation. */
	self->cctx = ZSTD_createCCtx();
	if (!self->cctx) {
		PyErr_NoMemory();
		return -1;
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
	ZSTD_CStream* cstream;
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

	cstream = CStream_from_ZstdCompressor(self, sourceSize);
	if (!cstream) {
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
			zresult = ZSTD_compressStream(cstream, &output, &input);
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
		zresult = ZSTD_endStream(cstream, &output);
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

	ZSTD_freeCStream(cstream);
	cstream = NULL;

	totalReadPy = PyLong_FromSsize_t(totalRead);
	totalWritePy = PyLong_FromSsize_t(totalWrite);
	res = PyTuple_Pack(2, totalReadPy, totalWritePy);
	Py_DecRef(totalReadPy);
	Py_DecRef(totalWritePy);

finally:
	if (output.dst) {
		PyMem_Free(output.dst);
	}

	if (cstream) {
		ZSTD_freeCStream(cstream);
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
	if (dictData && !self->cdict) {
		if (populate_cdict(self, dictData, dictSize, &zparams)) {
			Py_DECREF(output);
			return NULL;
		}
	}

	Py_BEGIN_ALLOW_THREADS
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
	ZstdCompressionObj* result = PyObject_New(ZstdCompressionObj, &ZstdCompressionObjType);
	if (!result) {
		return NULL;
	}

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "|n:compressobj", kwlist, &inSize)) {
		return NULL;
	}

	result->cstream = CStream_from_ZstdCompressor(self, inSize);
	if (!result->cstream) {
		Py_DECREF(result);
		return NULL;
	}

	result->output.dst = PyMem_Malloc(outSize);
	if (!result->output.dst) {
		PyErr_NoMemory();
		Py_DECREF(result);
		return NULL;
	}
	result->output.size = outSize;
	result->output.pos = 0;

	result->compressor = self;
	Py_INCREF(result->compressor);

	result->finished = 0;

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

	result = PyObject_New(ZstdCompressorIterator, &ZstdCompressorIteratorType);
	if (!result) {
		return NULL;
	}

	result->compressor = NULL;
	result->reader = NULL;
	result->buffer = NULL;
	result->cstream = NULL;
	result->input.src = NULL;
	result->output.dst = NULL;
	result->readResult = NULL;

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

		result->bufferOffset = 0;
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
	result->cstream = CStream_from_ZstdCompressor(self, sourceSize);
	if (!result->cstream) {
		goto except;
	}

	result->inSize = inSize;
	result->outSize = outSize;

	result->output.dst = PyMem_Malloc(outSize);
	if (!result->output.dst) {
		PyErr_NoMemory();
		goto except;
	}
	result->output.size = outSize;
	result->output.pos = 0;

	result->input.src = NULL;
	result->input.size = 0;
	result->input.pos = 0;

	result->finishedInput = 0;
	result->finishedOutput = 0;

	goto finally;

except:
	if (result->cstream) {
		ZSTD_freeCStream(result->cstream);
		result->cstream = NULL;
	}

	Py_DecRef((PyObject*)result->compressor);
	Py_DecRef(result->reader);

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

	result = PyObject_New(ZstdCompressionWriter, &ZstdCompressionWriterType);
	if (!result) {
		return NULL;
	}

	result->compressor = self;
	Py_INCREF(result->compressor);

	result->writer = writer;
	Py_INCREF(result->writer);

	result->sourceSize = sourceSize;

	result->outSize = outSize;

	result->entered = 0;
	result->cstream = NULL;

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
