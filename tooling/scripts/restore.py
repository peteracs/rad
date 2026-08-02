import os
import zlib
import struct

def read_index():
    with open('.git/index', 'rb') as f:
        data = f.read()
    
    # Parse git index
    # Header: 4 bytes signature, 4 bytes version, 4 bytes number of entries
    sig = data[0:4]
    version = struct.unpack('>I', data[4:8])[0]
    num_entries = struct.unpack('>I', data[8:12])[0]
    
    offset = 12
    for i in range(num_entries):
        # Entry: 
        # ctime (8), mtime (8), dev (4), ino (4), mode (4), uid (4), gid (4), size (4)
        # sha1 (20), flags (2)
        entry_start = offset
        ctime_s, ctime_n, mtime_s, mtime_n, dev, ino, mode, uid, gid, size = struct.unpack('>10I', data[offset:offset+40])
        offset += 40
        sha1 = data[offset:offset+20].hex()
        offset += 20
        flags = struct.unpack('>H', data[offset:offset+2])[0]
        offset += 2
        
        # Name
        name_end = data.find(b'\x00', offset)
        name = data[offset:name_end].decode('utf-8')
        offset = name_end + 1
        
        # Padding
        padding = 8 - ((offset - entry_start) % 8)
        if padding < 8:
            offset += padding
            
        if name.startswith('docs/src/') and name.endswith('.md'):
            print(f"Restoring {name} from {sha1}")
            restore_file(name, sha1)

def restore_file(name, sha1):
    obj_path = f".git/objects/{sha1[:2]}/{sha1[2:]}"
    if not os.path.exists(obj_path):
        print(f"Object {sha1} not found for {name}")
        return
        
    with open(obj_path, 'rb') as f:
        compressed = f.read()
        
    decompressed = zlib.decompress(compressed)
    
    # Format: "blob <size>\0<content>"
    null_idx = decompressed.find(b'\x00')
    content = decompressed[null_idx+1:]
    
    with open(name, 'wb') as f:
        f.write(content)

read_index()
