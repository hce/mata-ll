# mata-ll ZFS pool reader

A read-only ZFS pool reader, written in mata-ll. It reads files out of a ZFS
pool image by on-disk traversal — vdev label → highest-txg uberblock →
MOS objset → DSL dataset hierarchy → ZAP directories → dnode → block-pointer
and indirect-block reassembly — with per-structure endianness detection and
LZ4 decompression. It doubles as a worked example of a byte-level binary parser
in mata-ll, leaning on `LBit`, `ByteString`, and `LIO` raw file access.

## Scope

- **Single-disk and mirror (RAID1) vdevs only.** raidz is rejected by name.
  A mirror is read from one child (the copies are identical).
- **LZ4 compression only.** Other algorithms (zstd, gzip, lzjb, …) fail with a
  named error. Metadata is LZ4-compressed by default, so this is required to
  read anything.
- Filesystem datasets and their regular files; **no snapshots, no writes.**

## Running

```sh
# Read a pool image (one path; several paths for a mirror):
mll -r zpr.mll /path/to/pool.img
```

`zpr.mll` is a demonstration harness: it opens the pool, lists datasets and the
files in a dataset, and reconstructs each regular file to `/var/tmp/zpr-out/`
(writing through `LIOLinear`'s linear `%1` handle). The library API is:

```haskell
openPool     :: [String] -> IO (Either String Pool)
listDatasets :: Pool -> IO [String]
listFiles    :: Pool -> String -> IO [String]
readPath     :: Pool -> (String, String) -> IO (Either String ByteString)
                       -- (dataset name, path relative to the dataset root)
```

## Runtime requirement: Lua 5.4, **not** LuaJIT

This reader must run on **Lua 5.4** (e.g. mlua, `mll`'s default). It does **not**
work on LuaJIT.

ZFS is full of 64-bit on-disk values — DVA offsets, block-pointer words,
transaction group numbers. Lua 5.4 has native 64-bit integers and 64-bit
bitwise operators (`LBit`). LuaJIT has neither: numbers are doubles (a 53-bit
mantissa) and its `bit` library is 32-bit, so any value or shift that needs the
full 64 bits is silently corrupted.

The failure is not always silent. The block-pointer decode extracts the
top-level vdev index as `vdev = word0 >> 32` on a 64-bit word; under LuaJIT the
high bits are garbage, so a valid single-disk pool reports a bogus vdev number
and `openPool` fails with, for example:

```
openPool failed: blkptr references top-level vdev 8;
only single-vdev (disk or mirror) pools are supported
```

That is the 64-bit corruption surfacing as a plausible-looking wrong index (and
tripping the single-vdev guard), not a multi-vdev pool. Supporting LuaJIT
would require decoding the 64-bit words as split 32-bit halves, or via LuaJIT's
FFI `int64_t` — a deliberate non-goal here.

## Modules

| Module       | Role                                                                 |
|--------------|----------------------------------------------------------------------|
| `ZPool.mll`  | The reader and its public API (label → uberblock → MOS → DSL → ZAP → dnode → file). |
| `Lz4.mll`    | LZ4 block decompression plus ZFS's 4-byte big-endian length framing. |
| `Nvlist.mll` | XDR nvlist parser for the vdev-label pool config.                    |
| `ZBytes.mll` | Endian-parameterized integer and NUL-string readers.                 |
| `zpr.mll`    | Demonstration harness: list datasets/files and extract them to disk. |
