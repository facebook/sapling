Formatter output of a file whose content is not valid UTF-8

  $ newclientrepo
  >>> open("bin.png", "wb").write(b"\x89PNG\r\n\x1a\n\x93\xff\x00") and None
  >>> open("text.txt", "wb").write("caf\u00e9\n".encode()) and None
  $ hg commit -Aqm init

Control: UTF-8 text works with every formatter

  $ hg cat text.txt
  café
  $ hg cat -Tjson text.txt
  [
   {
    "abspath": "text.txt",
    "data": "caf\u00e9\n",
    "path": "text.txt"
   }
  ]
  $ hg cat -T '{abspath}: {data}' text.txt
  text.txt: café

Plain cat writes the exact bytes

  $ hg cat bin.png | f --hexdump -
  
  0000: 89 50 4e 47 0d 0a 1a 0a 93 ff 00                |.PNG.......|

BUG: the json and template formatters crash on the binary file, even when the
template does not reference {data}

  $ hg cat -Tjson bin.png 2>&1 | grep -E '^UnicodeDecodeError'
  UnicodeDecodeError: 'utf-8' codec can't decode byte 0x89 in position 0: invalid start byte
  $ hg cat -T '{abspath}\n' bin.png 2>&1 | grep -E '^UnicodeDecodeError'
  UnicodeDecodeError: 'utf-8' codec can't decode byte 0x89 in position 0: invalid start byte
  $ hg cat -T '{data}' bin.png 2>&1 | grep -E '^UnicodeDecodeError'
  UnicodeDecodeError: 'utf-8' codec can't decode byte 0x89 in position 0: invalid start byte
  $ hg cat -Tjson --output bin.json bin.png 2>&1 | grep -E '^UnicodeDecodeError'
  UnicodeDecodeError: 'utf-8' codec can't decode byte 0x89 in position 0: invalid start byte
