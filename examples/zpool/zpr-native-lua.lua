#!/usr/bin/env lua
-- zpr-fable.lua: a read-only ZFS pool reader in pure Lua 5.4.
--
-- Hand-written port of the mata-ll reader in this directory (ZPool.mll,
-- Lz4.mll, Nvlist.mll, ZBytes.mll, zpr.mll). It reads files out of a ZFS
-- pool image by on-disk traversal: vdev label nvlist -> uberblock ring
-- (highest txg wins) -> MOS objset -> DSL dataset hierarchy -> ZAP
-- directories -> dnodes -> (indirect) block-pointer reassembly, with
-- per-structure endianness detection and LZ4 decompression.
--
-- Scope (same as the original):
--   * single-disk and mirror vdevs only; raidz is rejected by name
--     (a mirror stores identical copies, so reads go to the first child)
--   * LZ4 and uncompressed blocks only; other compressors fail by name
--   * filesystem datasets and regular files; no snapshots, no writes
--   * checksums are not verified (reader for intact images)
--
-- Requires Lua 5.4 (or later), NOT LuaJIT: ZFS is full of 64-bit on-disk
-- values (DVA offsets, blkptr words, txgs) and this code depends on native
-- 64-bit integers and 64-bit bitwise operators. LuaJIT numbers are doubles
-- with a 32-bit `bit` library, which silently corrupts them.
--
-- Public API (mirrors ZPool.mll):
--   openPool(paths)          -> pool | nil, errstring   (paths: list; several = mirror)
--   listDatasets(pool)       -> { dataset name, ... }
--   listFiles(pool, dataset) -> { file path, ... }
--   readPath(pool, dataset, relpath) -> contents | nil, errstring
--
-- Run as a script it behaves like zpr.mll: opens the image(s) given as
-- arguments (default /var/tmp/bar.img), lists datasets and the files in
-- "foo/bar/baz", and reconstructs each regular file into /var/tmp/zpr-out/.

local sunpack = string.unpack

-- Raise an error without the "file:line:" prefix, so pcall-captured
-- messages read like the original's Either String results.
local function fail(msg)
    error(msg, 0)
end

-- ---------------------------------------------------------------------------
-- Byte readers (ZBytes.mll). All offsets are 0-based, as in the original;
-- conversion to Lua's 1-based strings happens only here.
-- ---------------------------------------------------------------------------

-- Endianness is represented directly as a string.unpack format prefix.
local LE, BE = "<", ">"

local function bbyte(b, off)  -- bsIndex
    return string.byte(b, off + 1)
end

local function bsub(b, off, len)  -- bsSub
    return string.sub(b, off + 1, off + len)
end

local function getU16(e, b, off)
    return (sunpack(e .. "I2", b, off + 1))
end

local function getU32(e, b, off)
    return (sunpack(e .. "I4", b, off + 1))
end

-- 64-bit values with the top bit set come out negative (native Lua 5.4
-- integers), which is fine: all consumers mask/shift rather than compare
-- magnitudes.
local function getU64(e, b, off)
    return (sunpack(e .. "I8", b, off + 1))
end

-- Split a buffer into consecutive big-endian integers of `width` bytes
-- (fat-ZAP value arrays store their integers big-endian on every host).
local function groupBE(width, b)
    local out, n, off = {}, #b, 0
    while off + width <= n do
        local acc = 0
        for i = 0, width - 1 do
            acc = (acc << 8) | string.byte(b, off + i + 1)
        end
        out[#out + 1] = acc
        off = off + width
    end
    return out
end

-- NUL-terminated string starting at `off`, at most `maxLen` bytes.
local function cstringAt(b, off, maxLen)
    local avail = #b - off
    local limit = maxLen < avail and maxLen or avail
    local len = limit
    local z = string.find(b, "\0", off + 1, true)
    if z and z - 1 - off < limit then
        len = z - 1 - off
    end
    return bsub(b, off, len)
end

-- ---------------------------------------------------------------------------
-- LZ4 (Lz4.mll): standard block decompression plus the ZFS framing.
-- ---------------------------------------------------------------------------

-- Decompress a raw LZ4 block to exactly `dlen` bytes (errors otherwise).
--
-- Output representation: since Lua strings are immutable, output is kept as
-- a flattened `window` string plus an ordered table of chunks appended since
-- the last match (never appended one byte at a time — no O(n^2) growth).
-- A match forces a flatten, because matches read from recent output;
-- literals just push a chunk. Blocks are at most 128K with a bounded number
-- of sequences, so the per-match flatten is cheap in practice.
local function lz4Decompress(src, dlen)
    local n = #src
    local ip = 0
    local window = ""
    local pending = {}

    -- Length-nibble extension: 15 means keep adding bytes, each 255
    -- continuing the run. Returns (length, next input offset).
    local function extend(nib, p)
        if nib ~= 15 then
            return nib, p
        end
        local acc = nib
        while true do
            if p >= n then
                fail("lz4: corrupt stream: truncated length extension")
            end
            local x = string.byte(src, p + 1)
            acc = acc + x
            p = p + 1
            if x ~= 255 then
                return acc, p
            end
        end
    end

    local function flatten()
        if #pending > 0 then
            window = window .. table.concat(pending)
            pending = {}
        end
    end

    while ip < n do
        local tok = string.byte(src, ip + 1)
        local litlen, ip1 = extend(tok >> 4, ip + 1)
        if litlen > 0 then
            pending[#pending + 1] = bsub(src, ip1, litlen)
        end
        local ip2 = ip1 + litlen
        if ip2 >= n then
            break  -- final sequence carries no match
        end
        local dist = getU16(LE, src, ip2)
        local mnib, ip3 = extend(tok & 15, ip2 + 2)
        local mlen = mnib + 4
        flatten()
        local start = #window - dist
        if dist <= 0 or start < 0 then
            fail("lz4: corrupt stream: match distance " .. dist
                 .. " with only " .. #window .. " bytes produced")
        end
        -- Copy `mlen` bytes starting `dist` back; an overlapping match
        -- (dist < mlen) repeats the available slice.
        local m
        if mlen <= dist then
            m = bsub(window, start, mlen)
        else
            local slice = bsub(window, start, dist)
            m = string.sub(string.rep(slice, mlen // dist + 1), 1, mlen)
        end
        pending[1] = m
        ip = ip3
    end

    flatten()
    if #window ~= dlen then
        fail("lz4: decompressed to " .. #window .. " bytes, expected " .. dlen)
    end
    return window
end

-- ZFS framing: 4-byte BIG-endian compressed length (the size of the raw LZ4
-- stream), then the stream itself; the physical block is padded out to the
-- allocated size, which is why the explicit length header is needed.
-- `dlen` is the expected decompressed (logical) size.
local function zfsLz4Decompress(payload, dlen)
    local total = #payload
    if total < 4 then
        fail("lz4: bad frame: compressed length ? in a " .. total .. "-byte payload")
    end
    local clen = getU32(BE, payload, 0)
    if clen < 0 or clen + 4 > total then
        fail("lz4: bad frame: compressed length " .. clen
             .. " in a " .. total .. "-byte payload")
    end
    return lz4Decompress(bsub(payload, 4, clen), dlen)
end

-- ---------------------------------------------------------------------------
-- Nvlist (Nvlist.mll): XDR nvlist parser for the vdev-label pool config.
-- All integers big-endian (XDR), regardless of pool byte order.
--
-- Values are tagged tables: {u64=n} | {str=s} | {list=pairs}
--   | {listarr={pairs,...}} | {bool=true} | {unknown=type}
-- and pairs are ordered arrays of {name, value}.
-- ---------------------------------------------------------------------------

local TY_BOOLEAN, TY_UINT64, TY_STRING, TY_NVLIST, TY_NVLIST_ARRAY =
    1, 8, 9, 19, 20

-- XDR string: uint32 byte length, bytes, padded to a multiple of 4.
local function xdrString(b, off)
    local n = getU32(BE, b, off)
    local padded = (n + 3) // 4 * 4
    return bsub(b, off + 4, n), off + 4 + padded
end

local parseNvl  -- forward declaration (mutual recursion with parseValue)

local function parseValue(b, off, typ, nelem, skipTo)
    if typ == TY_UINT64 then
        return { u64 = getU64(BE, b, off) }, off + 8
    elseif typ == TY_STRING then
        local s, o = xdrString(b, off)
        return { str = s }, o
    elseif typ == TY_NVLIST then
        local l, o = parseNvl(b, off)
        return { list = l }, o
    elseif typ == TY_NVLIST_ARRAY then
        local arr = {}
        local o = off
        for _ = 1, nelem do
            local l
            l, o = parseNvl(b, o)
            arr[#arr + 1] = l
        end
        return { listarr = arr }, o
    elseif typ == TY_BOOLEAN then
        return { bool = true }, skipTo
    else
        return { unknown = typ }, skipTo
    end
end

-- An nvlist body: int32 version, uint32 flags, then nvpairs. Each pair is
-- int32 encoded_size, int32 decoded_size (both zero terminates the list),
-- the name as an XDR string, int32 type, int32 element count, the value.
-- Types the pool config does not need are skipped via encoded_size, which
-- counts the whole pair in the XDR stream.
parseNvl = function(b, off)
    off = off + 8  -- version, flags
    local pairs_ = {}
    while true do
        local esz = getU32(BE, b, off)
        local dsz = getU32(BE, b, off + 4)
        if esz == 0 and dsz == 0 then
            return pairs_, off + 8
        end
        local name, o1 = xdrString(b, off + 8)
        local typ = getU32(BE, b, o1)
        local nelem = getU32(BE, b, o1 + 4)
        local v, nextOff = parseValue(b, o1 + 8, typ, nelem, off + esz)
        pairs_[#pairs_ + 1] = { name, v }
        off = nextOff
    end
end

-- Parse a label config nvlist, including its 4-byte encoding header
-- (encoding method, host endianness, two reserved bytes).
local function parseNvlist(b)
    if bbyte(b, 0) ~= 1 then
        fail("nvlist: unsupported encoding " .. bbyte(b, 0) .. " (expected XDR)")
    end
    return (parseNvl(b, 4))
end

local function nvLookup(ps, name)
    for _, p in ipairs(ps) do
        if p[1] == name then
            return p[2]
        end
    end
    return nil
end

-- Accessors that fail with a clear message when the key is missing or has
-- the wrong type — label configs missing these are unusable anyway.
local function nvString(ps, name)
    local v = nvLookup(ps, name)
    if v == nil or v.str == nil then
        fail("nvlist: missing string entry " .. name)
    end
    return v.str
end

local function nvU64(ps, name)
    local v = nvLookup(ps, name)
    if v == nil or v.u64 == nil then
        fail("nvlist: missing uint64 entry " .. name)
    end
    return v.u64
end

local function nvList(ps, name)
    local v = nvLookup(ps, name)
    if v == nil or v.list == nil then
        fail("nvlist: missing nvlist entry " .. name)
    end
    return v.list
end

-- ---------------------------------------------------------------------------
-- ZPool constants
-- ---------------------------------------------------------------------------

local UB_MAGIC         = 0x00bab10c
local VDEV_LABEL_START = 4194304   -- 4 MB reserved for L0/L1 + boot block
local LABEL_SIZE       = 262144    -- one vdev label: 256K
local UB_RING_SIZE     = 131072    -- uberblock ring: label bytes 128K..256K
local NVLIST_OFFSET    = 16384     -- config nvlist: label bytes 16K..128K
local NVLIST_SIZE      = 114688
local MASK48           = 0xffffffffffff      -- ZPL dirent object-id mask
local BNOT63           = 0x7fffffffffffffff  -- low 63 bits (blkptr offset)
local MZAP_BLOCK_TYPE  = 0x8000000000000003  -- wraps to the negative int
local FZAP_BLOCK_TYPE  = 0x8000000000000001  -- with the same bit pattern,
local ZBT_LEAF         = 0x8000000000000000  -- matching getU64's results
local SA_MAGIC         = 0x2F505A
local COMP_OFF         = 2
local COMP_LZ4         = 15
local OT_DSL_DIR       = 12  -- DMU_OT_DSL_DIR (bonus type)
local OT_DSL_DATASET   = 16  -- DMU_OT_DSL_DATASET (bonus type)
local OT_ZNODE         = 17  -- DMU_OT_ZNODE (pre-SA bonus type)
local OT_SA            = 44  -- DMU_OT_SA (system-attribute bonus)

-- ---------------------------------------------------------------------------
-- Device access
-- ---------------------------------------------------------------------------

-- pool.ctx = { devs = {file...}, e = LE|BE }: the opened image handles
-- (mirror children hold identical copies, so reads use the first) and the
-- pool's byte order.

local function readAt(f, off, len)
    f:seek("set", off)
    local s = f:read(len) or ""
    return s
end

local function devRead(ctx, off, len)
    local b = readAt(ctx.devs[1], off, len)
    if #b ~= len then
        fail("short read: wanted " .. len .. " bytes at offset " .. off
             .. ", got " .. #b)
    end
    return b
end

-- ---------------------------------------------------------------------------
-- Block pointers
-- ---------------------------------------------------------------------------

-- Blkptr representation:
--   { hole = true }
--   { embedded = true, comp = c, lsize = l, payload = bytes }
--   { vdev = v, offset = byteoff, gang = bool, comp = c, lsize = l, psize = p }

-- Embedded blkptrs (feature com.delphix:embedded_data) carry the block data
-- in the pointer itself: payload words at bytes 0..47, 56..79 and 88..127,
-- with sizes packed differently in props (lsize 25 bits, psize 7 bits).
local function parseEmbedded(e, b, props)
    if e == BE then
        fail("embedded blkptr on a byteswapped (big-endian) pool is not supported")
    end
    local lsize = (props & 0x1ffffff) + 1       -- low 25 bits
    local psize = ((props >> 25) & 0x7f) + 1    -- 7 bits
    local comp = (props >> 32) & 0x7f
    local payload = bsub(b, 0, 48) .. bsub(b, 56, 24) .. bsub(b, 88, 40)
    return { embedded = true, comp = comp, lsize = lsize,
             payload = bsub(payload, 0, psize) }
end

-- Parse a 128-byte blkptr. The props word (byte 48) carries, from low to
-- high bits: lsize, psize, compression (with the embedded flag at bit 39),
-- checksum, type, level, and the byte-order bit.
local function parseBp(e, b)
    local props = getU64(e, b, 48)
    if props == 0 then
        return { hole = true }
    end
    if (props >> 39) & 1 == 1 then
        return parseEmbedded(e, b, props)
    end
    local w0 = getU64(e, b, 0)
    local w1 = getU64(e, b, 8)
    local vdev = w0 >> 32
    local asize = (w0 & 0xffffff) << 9
    local gang = (w1 >> 63) == 1
    local offset = (w1 & BNOT63) << 9
    local lsize = ((props & 0xffff) + 1) << 9
    local psize = (((props >> 16) & 0xffff) + 1) << 9
    local comp = (props >> 32) & 0x7f
    if asize == 0 and offset == 0 then
        return { hole = true }
    end
    return { vdev = vdev, offset = offset, gang = gang,
             comp = comp, lsize = lsize, psize = psize }
end

local function compName(c)
    if c == 3 then return "lzjb"
    elseif c >= 5 and c <= 13 then return "gzip"
    elseif c == 14 then return "zle"
    elseif c == 16 then return "zstd"
    else return "code " .. c
    end
end

local function decompress(comp, raw, lsize)
    if comp == COMP_OFF then
        return bsub(raw, 0, lsize)
    elseif comp == COMP_LZ4 then
        return zfsLz4Decompress(raw, lsize)
    else
        fail("unsupported compression algorithm: " .. compName(comp)
             .. " (only lz4 and uncompressed blocks are supported)")
    end
end

-- Fetch and decompress the block a blkptr points at.
local function readBp(ctx, bp)
    if bp.hole then
        fail("readBp: hole blkptr (caller must handle holes)")
    end
    if bp.embedded then
        return decompress(bp.comp, bp.payload, bp.lsize)
    end
    if bp.gang then
        fail("gang blocks are not supported")
    end
    if bp.vdev ~= 0 then
        fail("blkptr references top-level vdev " .. bp.vdev
             .. "; only single-vdev (disk or mirror) pools are supported")
    end
    local raw = devRead(ctx, bp.offset + VDEV_LABEL_START, bp.psize)
    return decompress(bp.comp, raw, bp.lsize)
end

-- ---------------------------------------------------------------------------
-- Dnodes and object blocks
-- ---------------------------------------------------------------------------

local function parseDnode(e, b)
    local nbp = bbyte(b, 3)
    local blkptrs = {}
    for i = 0, nbp - 1 do
        blkptrs[i + 1] = parseBp(e, bsub(b, 64 + 128 * i, 128))
    end
    return {
        type        = bbyte(b, 0),
        indblkshift = bbyte(b, 1),
        nlevels     = bbyte(b, 2),
        nblkptr     = nbp,
        bonustype   = bbyte(b, 4),
        datablksz   = getU16(e, b, 8) * 512,
        maxblkid    = getU64(e, b, 16),
        blkptrs     = blkptrs,
        bonus       = bsub(b, 64 + 128 * nbp, getU16(e, b, 10)),
    }
end

-- Read logical block `blkid` of an object, walking the indirect levels.
-- Holes (at any level) read back as zeros of the data block size.
local function readBlock(ctx, dn, blkid)
    local epbs = dn.indblkshift - 7  -- log2 blkptrs per indirect block
    local top = blkid >> ((dn.nlevels - 1) * epbs)
    if top >= dn.nblkptr then
        fail("blkid " .. blkid .. " out of range for dnode with "
             .. dn.nblkptr .. " blkptrs")
    end
    local bp = dn.blkptrs[top + 1]
    local lvl = dn.nlevels - 2
    while true do
        if bp.hole then
            return string.rep("\0", dn.datablksz)
        end
        if lvl < 0 then
            return readBp(ctx, bp)
        end
        local d = readBp(ctx, bp)
        local idx = (blkid >> (lvl * epbs)) & ((1 << epbs) - 1)
        bp = parseBp(ctx.e, bsub(d, idx * 128, 128))
        lvl = lvl - 1
    end
end

-- An objset is represented by its meta dnode ({ meta = dnode }), whose data
-- blocks are the objset's dnode array (object K = 512-byte slot K).
-- Object K's dnode: slot K of the meta dnode's data (512 bytes per slot;
-- dn_extra_slots widens the slice for large dnodes).
local function getDnode(ctx, os_, objnum)
    local meta = os_.meta
    local bs = meta.datablksz
    local per = bs // 512
    local blk = readBlock(ctx, meta, objnum // per)
    local off = objnum % per * 512
    local extra = bbyte(blk, off + 12)
    local avail = bs - off
    local want = (1 + extra) * 512
    local size = want > avail and avail or want
    return parseDnode(ctx.e, bsub(blk, off, size))
end

-- ---------------------------------------------------------------------------
-- ZAP (both flavors)
-- ---------------------------------------------------------------------------

-- Microzap: a single block of 64-byte entries after a 64-byte header.
local function mzapEntries(e, blk)
    local out = {}
    for i = 1, #blk // 64 - 1 do
        local off = i * 64
        local name = cstringAt(blk, off + 14, 50)
        if name ~= "" then
            out[#out + 1] = { name, { getU64(e, blk, off) } }
        end
    end
    return out
end

-- One zap leaf: 48-byte header, hash table of blocksize/32 uint16s, then
-- 24-byte chunks. Entry chunks (type 252) name their string and value via
-- chains of array chunks (type 251, 21 payload bytes each). Fatzap array
-- integers are big-endian on every host.
local function leafEntries(e, bs, d)
    local chunksOff = 48 + bs // 16
    local nchunks = (bs - chunksOff) // 24

    local function readArray(ci, need)
        local parts = {}
        while need > 0 and ci ~= 65535 do
            local base = chunksOff + ci * 24
            if bbyte(d, base) ~= 251 then
                fail("zap leaf: broken array chunk chain")
            end
            local takeN = need < 21 and need or 21
            parts[#parts + 1] = bsub(d, base + 1, takeN)
            need = need - takeN
            ci = getU16(e, d, base + 22)
        end
        return table.concat(parts)
    end

    local out = {}
    for i = 0, nchunks - 1 do
        local base = chunksOff + i * 24
        if bbyte(d, base) == 252 then
            local intlen = bbyte(d, base + 1)
            local nameChunk = getU16(e, d, base + 4)
            local nameNum = getU16(e, d, base + 6)
            local valChunk = getU16(e, d, base + 8)
            local valNum = getU16(e, d, base + 10)
            local nameBytes = readArray(nameChunk, nameNum)
            local name = cstringAt(nameBytes, 0, #nameBytes)
            out[#out + 1] = { name, groupBE(intlen, readArray(valChunk, valNum * intlen)) }
        end
    end
    return out
end

-- Fatzap: walk every block of the object; blocks holding a leaf header
-- contribute their entry chunks. (Leaves are enumerated directly instead of
-- going through the hash pointer table — every leaf is stored exactly once,
-- and enumeration does not need the hash order.)
local function fzapEntries(ctx, dn)
    local out = {}
    for blkid = 1, dn.maxblkid do
        local d = readBlock(ctx, dn, blkid)
        if getU64(ctx.e, d, 0) == ZBT_LEAF then
            for _, ent in ipairs(leafEntries(ctx.e, dn.datablksz, d)) do
                out[#out + 1] = ent
            end
        end
    end
    return out
end

-- All entries of a ZAP object as an ordered list of { name, {int, ...} }.
-- Values are integer lists (directory ZAPs use a single uint64; SA layouts
-- use uint16 arrays).
local function zapEntries(ctx, os_, objnum)
    local dn = getDnode(ctx, os_, objnum)
    local blk0 = readBlock(ctx, dn, 0)
    local bt = getU64(ctx.e, blk0, 0)
    if bt == MZAP_BLOCK_TYPE then
        return mzapEntries(ctx.e, blk0)
    elseif bt == FZAP_BLOCK_TYPE then
        return fzapEntries(ctx, dn)
    else
        fail("object " .. objnum .. " is not a ZAP block (type " .. bt .. ")")
    end
end

local function assoc(name, ents)
    for _, ent in ipairs(ents) do
        if ent[1] == name then
            return ent[2]
        end
    end
    return nil
end

local function zapU64(what, ents, name)
    local vals = assoc(name, ents)
    if vals == nil or vals[1] == nil then
        fail(what .. ": missing ZAP key " .. name)
    end
    return vals[1]
end

-- ---------------------------------------------------------------------------
-- Pool open: labels, uberblocks, MOS
-- ---------------------------------------------------------------------------

-- Scan the uberblock rings of all four labels (two at the device start, two
-- at the end); each slot is 1 << max(ashift, 10) bytes. The byte order is
-- detected from the magic; the winner is the valid slot with the highest
-- txg. Returns (endian, txg, uberblock bytes).
local function scanUberblocks(f, size, ashift)
    local ubShift = ashift > 10 and ashift or 10
    local slotSize = 1 << ubShift
    local nslots = UB_RING_SIZE // slotSize
    local bases = {
        UB_RING_SIZE,                          -- L0 ring
        LABEL_SIZE + UB_RING_SIZE,             -- L1 ring
        size - 2 * LABEL_SIZE + UB_RING_SIZE,  -- L2 ring
        size - LABEL_SIZE + UB_RING_SIZE,      -- L3 ring
    }
    local best  -- { e, txg, bytes }
    for _, base in ipairs(bases) do
        local ring = readAt(f, base, UB_RING_SIZE)
        for i = 0, nslots - 1 do
            local off = i * slotSize
            if off + 168 <= #ring then
                local e
                if getU64(LE, ring, off) == UB_MAGIC then
                    e = LE
                elseif getU64(BE, ring, off) == UB_MAGIC then
                    e = BE
                end
                if e then
                    local txg = getU64(e, ring, off + 16)
                    if best == nil or txg > best.txg then
                        best = { e = e, txg = txg, bytes = bsub(ring, off, 168) }
                    end
                end
            end
        end
    end
    if best == nil then
        fail("no valid uberblock found in any label")
    end
    return best
end

local function openDev(path)
    local f, err = io.open(path, "rb")
    if not f then
        fail("cannot open " .. path .. ": " .. tostring(err))
    end
    return f
end

local function openPoolRaw(paths)
    local devs = {}
    for _, p in ipairs(paths) do
        devs[#devs + 1] = openDev(p)
    end
    local f = devs[1]
    local size = f:seek("end")
    if size < 2 * LABEL_SIZE + VDEV_LABEL_START then
        fail("image too small to be a vdev: " .. size .. " bytes")
    end
    -- config nvlist from label 0
    local nvb = readAt(f, NVLIST_OFFSET, NVLIST_SIZE)
    local cfg = parseNvlist(nvb)
    local name = nvString(cfg, "name")
    local vt = nvList(cfg, "vdev_tree")
    local vtype = nvString(vt, "type")
    local ashift = nvU64(vt, "ashift")
    if vtype ~= "disk" and vtype ~= "mirror" then
        fail("unsupported vdev type \"" .. vtype
             .. "\": only single disks and mirrors are supported")
    end
    if nvU64(cfg, "vdev_children") ~= 1 then
        fail("pools with multiple top-level vdevs are not supported")
    end
    -- best uberblock across all four labels
    local best = scanUberblocks(f, size, ashift)
    local ctx = { devs = devs, e = best.e }
    local rootbp = parseBp(best.e, bsub(best.bytes, 40, 128))
    local mosBytes = readBp(ctx, rootbp)
    local mos = { meta = parseDnode(best.e, bsub(mosBytes, 0, 512)) }
    local objdir = zapEntries(ctx, mos, 1)
    local rootDsl = zapU64("MOS object directory", objdir, "root_dataset")
    return { ctx = ctx, name = name, mos = mos, rootDsl = rootDsl }
end

-- openPool(paths) -> pool | nil, errstring. Several paths form a mirror.
local function openPool(paths)
    local ok, res = pcall(openPoolRaw, paths)
    if ok then
        return res
    end
    return nil, res
end

-- ---------------------------------------------------------------------------
-- DSL: dataset enumeration
-- ---------------------------------------------------------------------------

-- (head_dataset_obj, child_dir_zapobj) from a dsl_dir's bonus buffer.
local function dslDirInfo(pool, dirobj)
    local dn = getDnode(pool.ctx, pool.mos, dirobj)
    if dn.bonustype ~= OT_DSL_DIR then
        fail("object " .. dirobj .. " is not a DSL directory (bonus type "
             .. dn.bonustype .. ")")
    end
    return getU64(pool.ctx.e, dn.bonus, 8), getU64(pool.ctx.e, dn.bonus, 32)
end

local function sortByName(ents)
    table.sort(ents, function(a, b) return a[1] < b[1] end)
    return ents
end

-- Depth-first walk of the DSL directory tree; hidden internal directories
-- ($MOS, $FREE_DIR, $ORIGIN, ...) are skipped. Snapshots never appear here:
-- they live under ds_snapnames_zapobj, which this walk does not touch.
local function walkDsl(pool, dirobj, name, out)
    local headObj, childZap = dslDirInfo(pool, dirobj)
    out[#out + 1] = { name, headObj }
    local children = {}
    for _, ent in ipairs(zapEntries(pool.ctx, pool.mos, childZap)) do
        if string.byte(ent[1], 1) ~= 36 then  -- leading '$'
            children[#children + 1] = ent
        end
    end
    for _, ent in ipairs(sortByName(children)) do
        local obj = ent[2][1]
        if obj == nil then
            fail("empty child dir entry for " .. ent[1])
        end
        walkDsl(pool, obj, name .. "/" .. ent[1], out)
    end
    return out
end

local function allDatasets(pool)
    return walkDsl(pool, pool.rootDsl, pool.name, {})
end

local function listDatasets(pool)
    local out = {}
    for _, ds in ipairs(allDatasets(pool)) do
        out[#out + 1] = ds[1]
    end
    return out
end

-- The head dataset's objset: ds_bp lives at byte 128 of the dsl_dataset
-- bonus buffer and points at the filesystem's objset.
local function fsObjset(pool, dsobj)
    local ctx = pool.ctx
    local dn = getDnode(ctx, pool.mos, dsobj)
    if dn.bonustype ~= OT_DSL_DATASET then
        fail("object " .. dsobj .. " is not a DSL dataset (bonus type "
             .. dn.bonustype .. ")")
    end
    local osb = readBp(ctx, parseBp(ctx.e, bsub(dn.bonus, 128, 128)))
    return { meta = parseDnode(ctx.e, bsub(osb, 0, 512)) }
end

local function findDataset(pool, dsname)
    for _, ds in ipairs(allDatasets(pool)) do
        if ds[1] == dsname then
            return fsObjset(pool, ds[2])
        end
    end
    fail("no such dataset: " .. dsname)
end

-- ---------------------------------------------------------------------------
-- ZPL: files inside a filesystem
-- ---------------------------------------------------------------------------

-- ZPL directory entry: low 48 bits object id, top 4 bits the file type.
local DT_DIR, DT_REG = 4, 8

local function entryObj(v)
    return v & MASK48
end

local function entryType(v)
    return (v >> 60) & 15
end

-- Root directory object: master node (object 1) key "ROOT".
local function fsRootDir(pool, os_)
    local master = zapEntries(pool.ctx, os_, 1)
    return zapU64("ZPL master node", master, "ROOT")
end

local function walkDir(pool, os_, dirobj, prefix, out)
    for _, ent in ipairs(sortByName(zapEntries(pool.ctx, os_, dirobj))) do
        local v = ent[2][1]
        if entryType(v) == DT_DIR then
            walkDir(pool, os_, entryObj(v), prefix .. ent[1] .. "/", out)
        elseif entryType(v) == DT_REG then
            out[#out + 1] = prefix .. ent[1]
        end
    end
    return out
end

local function listFiles(pool, dsname)
    local os_ = findDataset(pool, dsname)
    return walkDir(pool, os_, fsRootDir(pool, os_), "", {})
end

-- Resolve a relative path (components already split) to a regular file's
-- object number.
local function resolvePath(pool, os_, dirobj, comps)
    if #comps == 0 then
        fail("readPath: empty path")
    end
    for i, c in ipairs(comps) do
        local ents = zapEntries(pool.ctx, os_, dirobj)
        local v = zapU64("directory", ents, c)
        if i == #comps then
            if entryType(v) ~= DT_REG then
                fail(c .. " is not a regular file")
            end
            return entryObj(v)
        end
        if entryType(v) ~= DT_DIR then
            fail(c .. " is not a directory")
        end
        dirobj = entryObj(v)
    end
end

-- ---------------------------------------------------------------------------
-- File sizes: system attributes (or the legacy znode bonus)
-- ---------------------------------------------------------------------------

-- Modern pools store the byte length as the ZPL_SIZE system attribute in
-- the dnode's SA bonus; the attribute layout is looked up from the
-- filesystem's SA registry (master node SA_ATTRS -> REGISTRY / LAYOUTS),
-- never assumed.
local function saSize(pool, os_, b)
    local e = pool.ctx.e
    if getU32(e, b, 0) ~= SA_MAGIC then
        fail("SA bonus magic mismatch: " .. getU32(e, b, 0))
    end
    local info = getU16(e, b, 4)
    local hdrsize = ((info >> 10) & 63) << 3
    local layoutnum = info & 1023
    local master = zapEntries(pool.ctx, os_, 1)
    local saMaster = zapEntries(pool.ctx, os_, zapU64("master node", master, "SA_ATTRS"))
    local reg = zapEntries(pool.ctx, os_, zapU64("SA master", saMaster, "REGISTRY"))
    local lays = zapEntries(pool.ctx, os_, zapU64("SA master", saMaster, "LAYOUTS"))
    local layout = assoc(tostring(layoutnum), lays)
    if layout == nil then
        fail("SA layout " .. layoutnum .. " not registered")
    end
    local sizeAttr = zapU64("SA registry", reg, "ZPL_SIZE") & 0xffff
    -- registry values encode (length << 24 | byteswap << 16 | attr number)
    local lengths = {}
    for _, ent in ipairs(reg) do
        local v = ent[2][1]
        if v == nil then
            fail("empty SA registry entry")
        end
        lengths[v & 0xffff] = (v >> 24) & 0xffff
    end
    -- walk the layout's attributes, summing lengths until ZPL_SIZE;
    -- variable-length attributes read their size from the SA header
    local off, varIdx = hdrsize, 0
    for _, anum in ipairs(layout) do
        if anum == sizeAttr then
            return getU64(e, b, off)
        end
        local l = lengths[anum]
        if l == nil then
            fail("SA attribute " .. anum .. " not in registry")
        end
        if l == 0 then
            off = off + getU16(e, b, 6 + 2 * varIdx)
            varIdx = varIdx + 1
        else
            off = off + l
        end
    end
    fail("SA layout has no ZPL_SIZE (spill blocks are not supported)")
end

-- The exact byte length of a file. Pre-SA pools (ZPL version <= 4) keep a
-- fixed znode_phys in the bonus with zp_size at byte 80.
local function fileSize(pool, os_, dn)
    if dn.bonustype == OT_ZNODE then
        return getU64(pool.ctx.e, dn.bonus, 80)
    elseif dn.bonustype == OT_SA then
        return saSize(pool, os_, dn.bonus)
    else
        fail("file dnode has unexpected bonus type " .. dn.bonustype)
    end
end

-- ---------------------------------------------------------------------------
-- readPath
-- ---------------------------------------------------------------------------

local function splitSlash(s)
    local comps = {}
    for c in string.gmatch(s, "[^/]+") do
        comps[#comps + 1] = c
    end
    return comps
end

local function readPathRaw(pool, dsname, relpath)
    local os_ = findDataset(pool, dsname)
    local root = fsRootDir(pool, os_)
    local fobj = resolvePath(pool, os_, root, splitSlash(relpath))
    local dn = getDnode(pool.ctx, os_, fobj)
    local size = fileSize(pool, os_, dn)
    local blocks = {}
    for blkid = 0, dn.maxblkid do
        blocks[#blocks + 1] = readBlock(pool.ctx, dn, blkid)
    end
    local whole = table.concat(blocks)
    if size > #whole then
        fail("file claims " .. size .. " bytes but blocks provide only " .. #whole)
    end
    return bsub(whole, 0, size)
end

-- readPath(pool, dataset, relpath) -> contents | nil, errstring
local function readPath(pool, dsname, relpath)
    local ok, res = pcall(readPathRaw, pool, dsname, relpath)
    if ok then
        return res
    end
    return nil, res
end

-- ---------------------------------------------------------------------------
-- Main: the zpr.mll demonstration harness
-- ---------------------------------------------------------------------------

-- Render a string list the way mll's `show` does: [a, b, c]
local function showStrList(list)
    return "[" .. table.concat(list, ", ") .. "]"
end

local function dump(pool, name)
    local bytes, err = readPath(pool, "foo/bar/baz", name)
    if not bytes then
        fail("readPath " .. name .. " failed: " .. err)
    end
    -- Files can live at nested paths (e.g. mllc/src/codegen.rs), so create
    -- the parent directory before opening the output for write.
    os.execute("mkdir -p \"$(dirname \"/var/tmp/zpr-out/" .. name .. "\")\"")
    local out, oerr = io.open("/var/tmp/zpr-out/" .. name, "wb")
    if not out then
        fail("cannot write output: " .. tostring(oerr))
    end
    out:write(bytes)
    out:close()
    print("wrote /var/tmp/zpr-out/" .. name .. " (" .. #bytes .. " bytes)")
end

local function main()
    local paths = {}
    for _, a in ipairs(arg or {}) do
        paths[#paths + 1] = a
    end
    if #paths == 0 then
        paths = { "/var/tmp/bar.img" }
    end
    local pool, err = openPool(paths)
    if not pool then
        fail("openPool failed: " .. err)
    end
    print("datasets: " .. showStrList(listDatasets(pool)))
    local files = listFiles(pool, "foo/bar/baz")
    print("files in foo/bar/baz: " .. showStrList(files))
    os.execute("mkdir -p /var/tmp/zpr-out")
    for _, name in ipairs(files) do
        dump(pool, name)
    end
end

main()
