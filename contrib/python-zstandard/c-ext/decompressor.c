/**
* Copyright (c) 2016-present, Gregory Szorc
* All rights reserved.
*
* This software may be modified and distributed under the terms
* of the BSD license. See the LICENSE file for details.
*/

#include "python-zstandard.h"

extern PyObject* ZstdError;

ZSTD_DStream* DStream_from_ZstdDecompressor(ZstdDecompressor* decompressor) {
	ZSTD_DStream* dstream;
	void* dictData = NULL;
	size_t dictSize = 0;
	size_t zresult;

	dstream = ZSTD_createDStream();
	if (!dstream) {
		PyErr_SetString(ZstdError, "could not create DStream");
		return NULL;
	}

	if (decompressor->dict) {
		dictData = decompressor->dict->dictData;
		dictSize = decompressor->dict->dictSize;
	}

	if (dictData) {
		zresult = ZSTD_initDStream_usingDict(dstream, dictData, dictSize);
	}
	else {
		zresult = ZSTD_initDStream(dstream);
	}

	if (ZSTD_isError(zresult)) {
		PyErr_Format(ZstdError, "could not initialize DStream: %s",
			ZSTD_getErrorName(zresult));
		return NULL;
	}

	return dstream;
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

	self->refdctx = NULL;
	self->dict = NULL;
	self->ddict = NULL;

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "|O!", kwlist,
		&ZstdCompressionDictType, &dict)) {
		return -1;
	}

	/* Instead of creating a ZSTD_DCtx for every decompression operation,
	   we create an instance at object creation time and recycle it via
	   ZSTD_copyDCTx() on each use. This means each use is a malloc+memcpy
	   instead of a malloc+init. */
	/* TODO lazily initialize the reference ZSTD_DCtx on first use since
	   not instances of ZstdDecompressor will use a ZSTD_DCtx. */
	self->refdctx = ZSTD_createDCtx();
	if (!self->refdctx) {
		PyErr_NoMemory();
		goto except;
	}

	if (dict) {
		self->dict = dict;
		Py_INCREF(dict);
	}

	return 0;

except:
	if (self->refdctx) {
		ZSTD_freeDCtx(self->refdctx);
		self->refdctx = NULL;
	}

	return -1;
}

static void Decompressor_dealloc(ZstdDecompressor* self) {
	if (self->refdctx) {
		ZSTD_freeDCtx(self->refdctx);
	}

	Py_XDECREF(self->dict);

	if (self->ddict) {
		ZSTD_freeDDict(self->ddict);
		self->ddict = NULL;
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
	ZSTD_DStream* dstream;
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

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "OO|kk", kwlist, &source,
		&dest, &inSize, &outSize)) {
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

	dstream = DStream_from_ZstdDecompressor(self);
	if (!dstream) {
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
			zresult = ZSTD_decompressStream(dstream, &output, &input);
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

	ZSTD_freeDStream(dstream);
	dstream = NULL;

	totalReadPy = PyLong_FromSsize_t(totalRead);
	totalWritePy = PyLong_FromSsize_t(totalWrite);
	res = PyTuple_Pack(2, totalReadPy, totalWritePy);
	Py_DecRef(totalReadPy);
	Py_DecRef(totalWritePy);

	finally:
	if (output.dst) {
		PyMem_Free(output.dst);
	}

	if (dstream) {
		ZSTD_freeDStream(dstream);
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
	ZSTD_DCtx* dctx = NULL;
	void* dictData = NULL;
	size_t dictSize = 0;
	size_t zresult;

#if PY_MAJOR_VERSION >= 3
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "y#|n", kwlist,
#else
	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "s#|n", kwlist,
#endif
		&source, &sourceSize, &maxOutputSize)) {
		return NULL;
	}

	dctx = PyMem_Malloc(ZSTD_sizeof_DCtx(self->refdctx));
	if (!dctx) {
		PyErr_NoMemory();
		return NULL;
	}

	ZSTD_copyDCtx(dctx, self->refdctx);

	if (self->dict) {
		dictData = self->dict->dictData;
		dictSize = self->dict->dictSize;
	}

	if (dictData && !self->ddict) {
		Py_BEGIN_ALLOW_THREADS
		self->ddict = ZSTD_createDDict(dictData, dictSize);
		Py_END_ALLOW_THREADS

		if (!self->ddict) {
			PyErr_SetString(ZstdError, "could not create decompression dict");
			goto except;
		}
	}

	decompressedSize = ZSTD_getDecompressedSize(source, sourceSize);
	/* 0 returned if content size not in the zstd frame header */
	if (0 == decompressedSize) {
		if (0 == maxOutputSize) {
			PyErr_SetString(ZstdError, "input data invalid or missing content size "
				"in frame header");
			goto except;
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
		goto except;
	}

	Py_BEGIN_ALLOW_THREADS
	if (self->ddict) {
		zresult = ZSTD_decompress_usingDDict(dctx, PyBytes_AsString(result), destCapacity,
			source, sourceSize, self->ddict);
	}
	else {
		zresult = ZSTD_decompressDCtx(dctx, PyBytes_AsString(result), destCapacity, source, sourceSize);
	}
	Py_END_ALLOW_THREADS

	if (ZSTD_isError(zresult)) {
		PyErr_Format(ZstdError, "decompression error: %s", ZSTD_getErrorName(zresult));
		goto except;
	}
	else if (decompressedSize && zresult != decompressedSize) {
		PyErr_Format(ZstdError, "decompression error: decompressed %zu bytes; expected %llu",
			zresult, decompressedSize);
		goto except;
	}
	else if (zresult < destCapacity) {
		if (_PyBytes_Resize(&result, zresult)) {
			goto except;
		}
	}

	goto finally;

except:
	Py_DecRef(result);
	result = NULL;

finally:
	if (dctx) {
		PyMem_FREE(dctx);
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
	ZstdDecompressionObj* result = PyObject_New(ZstdDecompressionObj, &ZstdDecompressionObjType);
	if (!result) {
		return NULL;
	}

	result->dstream = DStream_from_ZstdDecompressor(self);
	if (!result->dstream) {
		Py_DecRef((PyObject*)result);
		return NULL;
	}

	result->decompressor = self;
	Py_INCREF(result->decompressor);

	result->finished = 0;

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

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|kkk", kwlist, &reader,
		&inSize, &outSize, &skipBytes)) {
		return NULL;
	}

	if (skipBytes >= inSize) {
		PyErr_SetString(PyExc_ValueError,
			"skip_bytes must be smaller than read_size");
		return NULL;
	}

	result = PyObject_New(ZstdDecompressorIterator, &ZstdDecompressorIteratorType);
	if (!result) {
		return NULL;
	}

	result->decompressor = NULL;
	result->reader = NULL;
	result->buffer = NULL;
	result->dstream = NULL;
	result->input.src = NULL;
	result->output.dst = NULL;

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

		result->bufferOffset = 0;
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

	result->dstream = DStream_from_ZstdDecompressor(self);
	if (!result->dstream) {
		goto except;
	}

	result->input.src = PyMem_Malloc(inSize);
	if (!result->input.src) {
		PyErr_NoMemory();
		goto except;
	}
	result->input.size = 0;
	result->input.pos = 0;

	result->output.dst = NULL;
	result->output.size = 0;
	result->output.pos = 0;

	result->readCount = 0;
	result->finishedInput = 0;
	result->finishedOutput = 0;

	goto finally;

except:
	if (result->reader) {
		Py_DECREF(result->reader);
		result->reader = NULL;
	}

	if (result->buffer) {
		PyBuffer_Release(result->buffer);
		Py_DECREF(result->buffer);
		result->buffer = NULL;
	}

	Py_DECREF(result);
	result = NULL;

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

	if (!PyArg_ParseTupleAndKeywords(args, kwargs, "O|k", kwlist, &writer, &outSize)) {
		return NULL;
	}

	if (!PyObject_HasAttrString(writer, "write")) {
		PyErr_SetString(PyExc_ValueError, "must pass an object with a write() method");
		return NULL;
	}

	result = PyObject_New(ZstdDecompressionWriter, &ZstdDecompressionWriterType);
	if (!result) {
		return NULL;
	}

	result->decompressor = self;
	Py_INCREF(result->decompressor);

	result->writer = writer;
	Py_INCREF(result->writer);

	result->outSize = outSize;

	result->entered = 0;
	result->dstream = NULL;

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
