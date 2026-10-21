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

The json formatter carries undecodable bytes as UTF-8b surrogates (\udcXX),
so a consumer can recover the exact bytes

  $ hg cat -Tjson bin.png
  [
   {
    "abspath": "bin.png",
    "data": "\udc89PNG\r\n\u001a\n\udc93\udcff\u0000",
    "path": "bin.png"
   }
  ]
  $ hg cat -Tjson --output bin.json bin.png
  >>> import json
  >>> json.load(open("bin.json"))[0]["data"].encode("utf-8", "surrogateescape") == open("bin.png", "rb").read()
  True

The template formatter does not crash; a template that renders {data} shows
U+FFFD for each undecodable byte

  $ hg cat -T '{abspath}\n' bin.png
  bin.png
  $ hg cat -T '{data}' bin.png | f --hexdump -
  
  0000: ef bf bd 50 4e 47 0d 0a 1a 0a ef bf bd ef bf bd |...PNG..........|
  0010: 00                                              |.|
