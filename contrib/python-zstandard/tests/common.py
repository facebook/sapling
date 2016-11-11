import io

class OpCountingBytesIO(io.BytesIO):
    def __init__(self, *args, **kwargs):
        self._read_count = 0
        self._write_count = 0
        return super(OpCountingBytesIO, self).__init__(*args, **kwargs)

    def read(self, *args):
        self._read_count += 1
        return super(OpCountingBytesIO, self).read(*args)

    def write(self, data):
        self._write_count += 1
        return super(OpCountingBytesIO, self).write(data)
