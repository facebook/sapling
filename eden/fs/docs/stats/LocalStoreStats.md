# LocalStoreStats

1. `Duration get{xxx}{"local_store.get_{xxx}_us"}` :

The duration of fetching a xxx (blob, blobmetadata, tree) from Local Store

2. `Counter get{xxx}Success{"local_store.get_{xxx}_success"}` :

Count the number of xxx (blob, blobmetadata, tree) that are successfully fetched
from local store

3. `Counter get{xxx}Failure{"local_store.get_{xxx}_failure"}` :

Count the number of xxx (blob, blobmetadata, tree) that cannot get from local
store

4. `Counter get{xxx}Error{"local_store.get_{xxx}_error"}` :

Count the number of xxx (blob, blobmetadata, tree) that are fetched from local
store but it cannot get parsed.
