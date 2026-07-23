-- ZPool: a read-only ZFS pool reader.
--
-- Reads a pool from raw device image files by walking the real on-disk
-- structures: vdev label nvlist -> uberblock ring (highest txg wins) ->
-- MOS objset -> object directory -> DSL dir/dataset tree -> per-filesystem
-- objsets -> ZAP directories -> dnodes -> (indirect) block pointers.
--
-- Scope: single-disk and mirror pools (a mirror stores N identical full
-- copies, so reads go to the first child); LZ4 and uncompressed blocks.
-- Anything else (raidz/stripes, gang blocks, other compressors) fails with
-- a message naming the unsupported feature. Snapshots are never entered:
-- dataset enumeration follows dd_child_dir_zapobj and dd_head_dataset_obj
-- only, which reach exactly the live filesystems.
--
-- Checksums are not verified — this is a reader for intact images, and
-- structural validation (magics, types, sizes) catches gross corruption.

module ZPool
    ( Pool
    , openPool
    , listDatasets
    , listFiles
    , readPath
    ) where

import LBit (band, bor, shiftL, shiftR)
import LIO (FileHandle, fOpen, fSeek, fReadN)
import LString (strByte, strSub, strLen)
import Data.List (sortBy)
import ZBytes (Endian(..), getU16, getU32, getU64, getU64LE, getU64BE, groupBE, cstringAt)
import Lz4 (zfsLz4Decompress)
import Nvlist (parseNvlist, nvString, nvU64, nvList)

-- ---------------------------------------------------------------------------
-- Constants (the lexer has no hex literals; decimal values are annotated)
-- ---------------------------------------------------------------------------

ubMagic :: Int
ubMagic = 12235020                   -- 0x00bab10c

vdevLabelStart :: Int
vdevLabelStart = 4194304             -- 4 MB reserved for L0/L1 + boot block

labelSize :: Int
labelSize = 262144                   -- one vdev label: 256K

ubRingSize :: Int
ubRingSize = 131072                  -- uberblock ring: label bytes 128K..256K

nvlistOffset :: Int
nvlistOffset = 16384                 -- config nvlist: label bytes 16K..128K

nvlistSize :: Int
nvlistSize = 114688

mask48 :: Int
mask48 = 281474976710655             -- 2^48 - 1 (ZPL directory entry object id)

-- 2^63 - 1 without hex literals: (1 << 63) wraps to the minimum 64-bit
-- integer, and subtracting 1 wraps again to the maximum — the low 63 bits.
bnot63 :: Int
bnot63 = shiftL 1 63 - 1

mzapBlockType :: Int
mzapBlockType = bor (shiftL 1 63) 3  -- 0x8000000000000003

fzapBlockType :: Int
fzapBlockType = bor (shiftL 1 63) 1  -- 0x8000000000000001

zbtLeaf :: Int
zbtLeaf = shiftL 1 63                -- 0x8000000000000000

saMagic :: Int
saMagic = 3100762                    -- 0x2F505A

compOff :: Int
compOff = 2

compLz4 :: Int
compLz4 = 15

otDslDir :: Int
otDslDir = 12                        -- DMU_OT_DSL_DIR (bonus type)

otDslDataset :: Int
otDslDataset = 16                    -- DMU_OT_DSL_DATASET (bonus type)

otZnode :: Int
otZnode = 17                         -- DMU_OT_ZNODE (pre-SA bonus type)

otSa :: Int
otSa = 44                            -- DMU_OT_SA (system-attribute bonus)

-- ---------------------------------------------------------------------------
-- Types
-- ---------------------------------------------------------------------------

-- Device context: the opened image handles (mirror children hold identical
-- copies, so reads use the first) and the pool's byte order.
data Ctx = Ctx [FileHandle] Endian

ctxEndian :: Ctx -> Endian
ctxEndian (Ctx _ e) = e

data Pool = Pool { poolCtx :: Ctx
                 , poolName :: String
                 , poolMos :: Objset
                 , poolRootDsl :: Int
                 }

-- An objset is represented by its meta dnode, whose data blocks are the
-- objset's dnode array (object K = 512-byte slot K).
data Objset = Objset Dnode

data Dva = Dva Int Int Bool          -- vdev id, byte offset, gang bit

data Blkptr = BpHole
            | BpEmbedded Int Int ByteString  -- comp, lsize, payload
            | Bp Dva Int Int Int         -- dva, comp, lsize, psize

data Dnode = Dnode { dnType :: Int
                   , dnIndBlkShift :: Int
                   , dnNLevels :: Int
                   , dnNBlkptr :: Int
                   , dnBonusType :: Int
                   , dnDataBlkSz :: Int
                   , dnMaxBlkId :: Int
                   , dnBlkptrs :: [Blkptr]
                   , dnBonus :: ByteString
                   }

-- ---------------------------------------------------------------------------
-- Device access
-- ---------------------------------------------------------------------------

devRead :: Ctx -> Int -> Int -> IO ByteString
devRead (Ctx devs _) off len = do
    let h = head devs
    _ <- fSeek h "set" off
    s <- fReadN h len
    let b = bsFromString s
    if bsLength b /= len
    then error ("short read: wanted " <> show len <> " bytes at offset "
                <> show off <> ", got " <> show (bsLength b))
    else pure b

-- ---------------------------------------------------------------------------
-- Block pointers
-- ---------------------------------------------------------------------------

-- Parse a 128-byte blkptr. The props word (byte 48) carries, from low to
-- high bits: lsize, psize, compression (with the embedded flag at bit 39),
-- checksum, type, level, and the byte-order bit.
parseBp :: Endian -> ByteString -> Blkptr
parseBp end b =
    let props = getU64 end b 48
    in if props == 0
       then BpHole
       else if band (shiftR props 39) 1 == 1
       then parseEmbedded end b props
       else
           let w0 = getU64 end b 0
               w1 = getU64 end b 8
               vdev = shiftR w0 32
               asize = shiftL (band w0 16777215) 9
               gang = shiftR w1 63
               offset = shiftL (band w1 bnot63) 9
               lsize = shiftL (band props 65535 + 1) 9
               psize = shiftL (band (shiftR props 16) 65535 + 1) 9
               comp = band (shiftR props 32) 127
           in if asize == 0 && offset == 0
              then BpHole
              else Bp (Dva vdev offset (gang == 1)) comp lsize psize

-- Embedded blkptrs (feature com.delphix:embedded_data) carry the block
-- data in the pointer itself: payload words at bytes 0..47, 56..79 and
-- 88..127, with sizes packed differently in props (lsize 25 bits,
-- psize 7 bits).
parseEmbedded :: Endian -> ByteString -> Int -> Blkptr
parseEmbedded end b props = case end of
    BE -> error "embedded blkptr on a byteswapped (big-endian) pool is not supported"
    LE ->
        let lsize = band props 33554431 + 1          -- low 25 bits
            psize = band (shiftR props 25) 127 + 1   -- 7 bits
            comp = band (shiftR props 32) 127
            payload = bsConcatList
                [ bsSub b 0 48
                , bsSub b 56 24
                , bsSub b 88 40
                ]
        in BpEmbedded comp lsize (bsSub payload 0 psize)

compName :: Int -> String
compName c =
    if c == 3 then "lzjb"
    else if c >= 5 && c <= 13 then "gzip"
    else if c == 14 then "zle"
    else if c == 16 then "zstd"
    else "code " <> show c

decompress :: Int -> ByteString -> Int -> ByteString
decompress comp raw lsize =
    if comp == compOff
    then bsSub raw 0 lsize
    else if comp == compLz4
    then zfsLz4Decompress raw lsize
    else error ("unsupported compression algorithm: " <> compName comp
                <> " (only lz4 and uncompressed blocks are supported)")

-- Fetch and decompress the block a blkptr points at.
readBp :: Ctx -> Blkptr -> IO ByteString
readBp ctx bp = case bp of
    BpHole -> error "readBp: hole blkptr (caller must handle holes)"
    BpEmbedded comp lsize payload -> pure (decompress comp payload lsize)
    Bp (Dva vdev offset gang) comp lsize psize ->
        if gang
        then error "gang blocks are not supported"
        else if vdev /= 0
        then error ("blkptr references top-level vdev " <> show vdev
                    <> "; only single-vdev (disk or mirror) pools are supported")
        else do
            raw <- devRead ctx (offset + vdevLabelStart) psize
            pure (decompress comp raw lsize)

-- ---------------------------------------------------------------------------
-- Dnodes and object blocks
-- ---------------------------------------------------------------------------

parseDnode :: Endian -> ByteString -> Dnode
parseDnode end b = Dnode { dnType = bsIndex b 0
                         , dnIndBlkShift = bsIndex b 1
                         , dnNLevels = bsIndex b 2
                         , dnNBlkptr = nbp
                         , dnBonusType = bsIndex b 4
                         , dnDataBlkSz = getU16 end b 8 * 512
                         , dnMaxBlkId = getU64 end b 16
                         , dnBlkptrs = map (\i -> parseBp end (bsSub b (64 + 128 * i) 128))
                                           [0 .. nbp - 1]
                         , dnBonus = bsSub b (64 + 128 * nbp) (getU16 end b 10)
                         }
  where
    nbp = bsIndex b 3

-- Read logical block `blkid` of an object, walking the indirect levels.
-- Holes (at any level) read back as zeros of the data block size.
readBlock :: Ctx -> Dnode -> Int -> IO ByteString
readBlock ctx dn blkid = do
    let levels = dnNLevels dn
        epbs = dnIndBlkShift dn - 7          -- log2 blkptrs per indirect blk
        top = shiftR blkid ((levels - 1) * epbs)
    if top >= dnNBlkptr dn
    then error ("blkid " <> show blkid <> " out of range for dnode with "
                <> show (dnNBlkptr dn) <> " blkptrs")
    else descend (dnBlkptrs dn !! top) (levels - 2)
  where
    epbs = dnIndBlkShift dn - 7
    descend bp lvl = case bp of
        BpHole -> pure (bsReplicate (dnDataBlkSz dn) 0)
        _ ->
            if lvl < 0
            then readBp ctx bp
            else do
                d <- readBp ctx bp
                let idx = band (shiftR blkid (lvl * epbs)) (shiftL 1 epbs - 1)
                descend (parseBp (ctxEndian ctx) (bsSub d (idx * 128) 128)) (lvl - 1)

-- Object K's dnode: slot K of the meta dnode's data (512 bytes per slot;
-- dn_extra_slots widens the slice for large dnodes).
getDnode :: Ctx -> Objset -> Int -> IO Dnode
getDnode ctx (Objset meta) objnum = do
    let bs = dnDataBlkSz meta
        per = div bs 512
    blk <- readBlock ctx meta (div objnum per)
    let off = mod objnum per * 512
        extra = bsIndex blk (off + 12)
        avail = bs - off
        want = (1 + extra) * 512
        size = if want > avail then avail else want
    pure (parseDnode (ctxEndian ctx) (bsSub blk off size))

-- ---------------------------------------------------------------------------
-- ZAP (both flavors)
-- ---------------------------------------------------------------------------

-- All entries of a ZAP object as (name, value ints). Values are returned as
-- integer lists (directory ZAPs use a single uint64; SA layouts use uint16
-- arrays).
zapEntries :: Ctx -> Objset -> Int -> IO [(String, [Int])]
zapEntries ctx os objnum = do
    dn <- getDnode ctx os objnum
    blk0 <- readBlock ctx dn 0
    let end = ctxEndian ctx
        bt = getU64 end blk0 0
    if bt == mzapBlockType
    then pure (mzapEntries end blk0)
    else if bt == fzapBlockType
    then fzapEntries ctx dn
    else error ("object " <> show objnum <> " is not a ZAP block (type "
                <> show bt <> ")")

-- Microzap: a single block of 64-byte entries after a 64-byte header.
mzapEntries :: Endian -> ByteString -> [(String, [Int])]
mzapEntries end blk = concatMap slot [1 .. div (bsLength blk) 64 - 1]
  where
    slot i =
        let off = i * 64
            value = getU64 end blk off
            name = cstringAt blk (off + 14) 50
        in if name == "" then [] else [(name, [value])]

-- Fatzap: walk every block of the object; blocks holding a leaf header
-- contribute their entry chunks. (Leaves are enumerated directly instead of
-- going through the hash pointer table — every leaf is stored exactly once,
-- and enumeration does not need the hash order.)
fzapEntries :: Ctx -> Dnode -> IO [(String, [Int])]
fzapEntries ctx dn = do
    parts <- mapM leafBlock [1 .. dnMaxBlkId dn]
    pure (concat parts)
  where
    end = ctxEndian ctx
    leafBlock blkid = do
        d <- readBlock ctx dn blkid
        if getU64 end d 0 /= zbtLeaf
        then pure []
        else pure (leafEntries end (dnDataBlkSz dn) d)

-- One zap leaf: 48-byte header, hash table of blocksize/32 uint16s, then
-- 24-byte chunks. Entry chunks (type 252) name their string and value via
-- chains of array chunks (type 251, 21 payload bytes each). Fatzap array
-- integers are big-endian on every host.
leafEntries :: Endian -> Int -> ByteString -> [(String, [Int])]
leafEntries end bs d = concatMap chunkEntry [0 .. nchunks - 1]
  where
    chunksOff = 48 + div bs 16
    nchunks = div (bs - chunksOff) 24
    chunkEntry i =
        let base = chunksOff + i * 24
        in if bsIndex d base /= 252
           then []
           else
               let intlen = bsIndex d (base + 1)
                   nameChunk = getU16 end d (base + 4)
                   nameNum = getU16 end d (base + 6)
                   valChunk = getU16 end d (base + 8)
                   valNum = getU16 end d (base + 10)
                   nameBytes = readArray nameChunk nameNum
                   name = cstringAt nameBytes 0 (bsLength nameBytes)
                   vals = groupBE intlen (readArray valChunk (valNum * intlen))
               in [(name, vals)]
    readArray ci need =
        if need <= 0 || ci == 65535
        then bsEmpty
        else
            let base = chunksOff + ci * 24
                takeN = if need < 21 then need else 21
            in if bsIndex d base /= 251
               then error "zap leaf: broken array chunk chain"
               else bsConcat (bsSub d (base + 1) takeN)
                             (readArray (getU16 end d (base + 22)) (need - takeN))

zapU64 :: String -> [(String, [Int])] -> String -> Int
zapU64 what ents name = case assoc name ents of
    Just (v : _) -> v
    _ -> error (what <> ": missing ZAP key " <> name)

assoc :: String -> [(String, a)] -> Maybe a
assoc _ [] = Nothing
assoc name ((k, v) : rest) = if k == name then Just v else assoc name rest

-- ---------------------------------------------------------------------------
-- Pool open: labels, uberblocks, MOS
-- ---------------------------------------------------------------------------

openPool :: [String] -> IO (Either String Pool)
openPool paths = try (do
    pool <- openPool' paths
    -- force the lazily-built fields inside the try so parse errors become
    -- Left instead of escaping as thunks (mata-ll pure values are lazy,
    -- exactly like GHC's `try (evaluate ...)` pattern)
    seq (strLen (poolName pool)) (seq (poolRootDsl pool) (pure pool)))

openPool' :: [String] -> IO Pool
openPool' paths = do
    devs <- mapM openDev paths
    let h = head devs
    size <- fSeek h "end" 0
    if size < 2 * labelSize + vdevLabelStart
    then error ("image too small to be a vdev: " <> show size <> " bytes")
    else pure ()
    -- config nvlist from label 0
    nvb <- devReadH h nvlistOffset nvlistSize
    let cfg = parseNvlist nvb
        name = nvString cfg "name"
        vt = nvList cfg "vdev_tree"
        vtype = nvString vt "type"
        ashift = nvU64 vt "ashift"
    if vtype == "disk" || vtype == "mirror"
    then pure ()
    else error ("unsupported vdev type \"" <> vtype
                <> "\": only single disks and mirrors are supported")
    if nvU64 cfg "vdev_children" == 1
    then pure ()
    else error "pools with multiple top-level vdevs are not supported"
    -- best uberblock across all four labels
    best <- scanUberblocks h size ashift
    case best of
        (end, txg, ubBytes) -> do
            let rootbp = parseBp end (bsSub ubBytes 40 128)
                ctx = Ctx devs end
            txg `seq` pure ()
            mosBytes <- readBp ctx rootbp
            let mos = Objset (parseDnode end (bsSub mosBytes 0 512))
            objdir <- zapEntries ctx mos 1
            let rootDsl = zapU64 "MOS object directory" objdir "root_dataset"
            pure (Pool { poolCtx = ctx, poolName = name, poolMos = mos
                       , poolRootDsl = rootDsl })

openDev :: String -> IO FileHandle
openDev path = do
    r <- fOpen path "rb"
    case r of
        Left err -> error ("cannot open " <> path <> ": " <> err)
        Right h -> pure h

devReadH :: FileHandle -> Int -> Int -> IO ByteString
devReadH h off len = do
    _ <- fSeek h "set" off
    s <- fReadN h len
    pure (bsFromString s)

-- Scan the uberblock rings of all four labels (two at the device start, two
-- at the end); each slot is 1 << max(ashift, 10) bytes. The byte order is
-- detected from the magic; the winner is the valid slot with the highest txg.
scanUberblocks :: FileHandle -> Int -> Int -> IO (Endian, Int, ByteString)
scanUberblocks h size ashift = do
    let ubShift = if ashift > 10 then ashift else 10
        slotSize = shiftL 1 ubShift
        nslots = div ubRingSize slotSize
        bases = [ ubRingSize                            -- L0 ring
                , labelSize + ubRingSize                -- L1 ring
                , size - 2 * labelSize + ubRingSize     -- L2 ring
                , size - labelSize + ubRingSize         -- L3 ring
                ]
    rings <- mapM (\base -> devReadH h base ubRingSize) bases
    let candidates = concatMap (ringSlots slotSize nslots) rings
    case candidates of
        [] -> error "no valid uberblock found in any label"
        (c : cs) -> pure (bestUb c cs)
  where
    ringSlots slotSize nslots ring = concatMap (slotAt ring slotSize) [0 .. nslots - 1]
    slotAt ring slotSize i =
        let off = i * slotSize
            magicLe = getU64LE ring off
            magicBe = getU64BE ring off
        in if magicLe == ubMagic
           then [(LE, getU64LE ring (off + 16), bsSub ring off 168)]
           else if magicBe == ubMagic
           then [(BE, getU64BE ring (off + 16), bsSub ring off 168)]
           else []
    bestUb best [] = best
    bestUb best (c : cs) =
        let (_, bestTxg, _) = best
            (_, txg, _) = c
        in if txg > bestTxg then bestUb c cs else bestUb best cs

-- ---------------------------------------------------------------------------
-- DSL: dataset enumeration
-- ---------------------------------------------------------------------------

-- (head_dataset_obj, child_dir_zapobj) from a dsl_dir's bonus buffer.
dslDirInfo :: Pool -> Int -> IO (Int, Int)
dslDirInfo pool dirobj = do
    dn <- getDnode (poolCtx pool) (poolMos pool) dirobj
    if dnBonusType dn /= otDslDir
    then error ("object " <> show dirobj <> " is not a DSL directory (bonus type "
                <> show (dnBonusType dn) <> ")")
    else pure ()
    let b = dnBonus dn
        end = ctxEndian (poolCtx pool)
    pure (getU64 end b 8, getU64 end b 32)

-- Depth-first walk of the DSL directory tree; hidden internal directories
-- ($MOS, $FREE_DIR, $ORIGIN, ...) are skipped. Snapshots never appear here:
-- they live under ds_snapnames_zapobj, which this walk does not touch.
walkDsl :: Pool -> Int -> String -> IO [(String, Int)]
walkDsl pool dirobj name = do
    hd <- dslDirInfo pool dirobj
    ents <- zapEntries (poolCtx pool) (poolMos pool) (snd hd)
    children <- mapM child (sortByName (filter visible ents))
    pure ((name, fst hd) : concat children)
  where
    visible (nm, _) = strByte nm 1 /= 36           -- leading '$'
    child (cname, vals) = case vals of
        (obj : _) -> walkDsl pool obj (name <> "/" <> cname)
        [] -> error ("empty child dir entry for " <> cname)

sortByName :: [(String, a)] -> [(String, a)]
sortByName = sortBy cmp
  where
    cmp (a, _) (b, _) = if a < b then LT else if a > b then GT else EQ

allDatasets :: Pool -> IO [(String, Int)]
allDatasets pool = walkDsl pool (poolRootDsl pool) (poolName pool)

listDatasets :: Pool -> IO [String]
listDatasets pool = do
    ds <- allDatasets pool
    pure (map fst ds)

-- The head dataset's objset: ds_bp lives at byte 128 of the dsl_dataset
-- bonus buffer and points at the filesystem's objset.
fsObjset :: Pool -> Int -> IO Objset
fsObjset pool dsobj = do
    let ctx = poolCtx pool
        end = ctxEndian ctx
    dn <- getDnode ctx (poolMos pool) dsobj
    if dnBonusType dn /= otDslDataset
    then error ("object " <> show dsobj <> " is not a DSL dataset (bonus type "
                <> show (dnBonusType dn) <> ")")
    else pure ()
    osb <- readBp ctx (parseBp end (bsSub (dnBonus dn) 128 128))
    pure (Objset (parseDnode end (bsSub osb 0 512)))

findDataset :: Pool -> String -> IO Objset
findDataset pool dsname = do
    ds <- allDatasets pool
    case assoc dsname ds of
        Just obj -> fsObjset pool obj
        Nothing -> error ("no such dataset: " <> dsname)

-- ---------------------------------------------------------------------------
-- ZPL: files inside a filesystem
-- ---------------------------------------------------------------------------

-- ZPL directory entry: low 48 bits object id, top 4 bits the file type.
dtDir :: Int
dtDir = 4

dtReg :: Int
dtReg = 8

entryObj :: Int -> Int
entryObj v = band v mask48

entryType :: Int -> Int
entryType v = band (shiftR v 60) 15

-- Root directory object: master node (object 1) key "ROOT".
fsRootDir :: Pool -> Objset -> IO Int
fsRootDir pool os = do
    master <- zapEntries (poolCtx pool) os 1
    pure (zapU64 "ZPL master node" master "ROOT")

listFiles :: Pool -> String -> IO [String]
listFiles pool dsname = do
    os <- findDataset pool dsname
    root <- fsRootDir pool os
    walkDir os root ""
  where
    walkDir os dirobj prefix = do
        ents <- zapEntries (poolCtx pool) os dirobj
        parts <- mapM (entry os prefix) (sortByName ents)
        pure (concat parts)
    entry os prefix (name, vals) = do
        let v = head vals
        if entryType v == dtDir
        then walkDir os (entryObj v) (prefix <> name <> "/")
        else if entryType v == dtReg
        then pure [prefix <> name]
        else pure []

-- Resolve a relative path (components already split) to a regular file's
-- object number.
resolvePath :: Pool -> Objset -> Int -> [String] -> IO Int
resolvePath _ _ _ [] = error "readPath: empty path"
resolvePath pool os dirobj (c : rest) = do
    ents <- zapEntries (poolCtx pool) os dirobj
    let v = zapU64 "directory" ents c
    if null rest
    then if entryType v == dtReg
         then pure (entryObj v)
         else error (c <> " is not a regular file")
    else if entryType v == dtDir
         then resolvePath pool os (entryObj v) rest
         else error (c <> " is not a directory")

-- ---------------------------------------------------------------------------
-- File sizes: system attributes (or the legacy znode bonus)
-- ---------------------------------------------------------------------------

-- The exact byte length of a file. Modern pools store it as the ZPL_SIZE
-- system attribute in the dnode's SA bonus; the attribute layout is looked
-- up from the filesystem's SA registry (master node SA_ATTRS -> REGISTRY /
-- LAYOUTS), never assumed. Pre-SA pools (ZPL version <= 4) keep a fixed
-- znode_phys in the bonus with zp_size at byte 80.
fileSize :: Pool -> Objset -> Dnode -> IO Int
fileSize pool os dn =
    let end = ctxEndian (poolCtx pool)
        b = dnBonus dn
    in if dnBonusType dn == otZnode
       then pure (getU64 end b 80)
       else if dnBonusType dn == otSa
       then saSize pool os b
       else error ("file dnode has unexpected bonus type "
                   <> show (dnBonusType dn))

saSize :: Pool -> Objset -> ByteString -> IO Int
saSize pool os b = do
    let end = ctxEndian (poolCtx pool)
    if getU32 end b 0 /= saMagic
    then error ("SA bonus magic mismatch: " <> show (getU32 end b 0))
    else pure ()
    let info = getU16 end b 4
        hdrsize = shiftL (band (shiftR info 10) 63) 3
        layoutnum = band info 1023
    master <- zapEntries (poolCtx pool) os 1
    saMaster <- zapEntries (poolCtx pool) os (zapU64 "master node" master "SA_ATTRS")
    reg <- zapEntries (poolCtx pool) os (zapU64 "SA master" saMaster "REGISTRY")
    lays <- zapEntries (poolCtx pool) os (zapU64 "SA master" saMaster "LAYOUTS")
    let layout = case assoc (show layoutnum) lays of
            Just l -> l
            Nothing -> error ("SA layout " <> show layoutnum <> " not registered")
        sizeAttr = band (zapU64 "SA registry" reg "ZPL_SIZE") 65535
    pure (walkLayout end sizeAttr (regLengths reg) layout hdrsize 0)
  where
    -- registry values encode (length << 24 | byteswap << 16 | attr number)
    regLengths reg = map (\(_, vals) -> case vals of
        (v : _) -> (band v 65535, band (shiftR v 24) 65535)
        [] -> error "empty SA registry entry") reg
    attrLen table anum = case lookupNum table anum of
        Just l -> l
        Nothing -> error ("SA attribute " <> show anum <> " not in registry")
    lookupNum [] _ = Nothing
    lookupNum ((k, l) : rest) anum = if k == anum then Just l else lookupNum rest anum
    -- walk the layout's attributes, summing lengths until ZPL_SIZE;
    -- variable-length attributes read their size from the SA header
    walkLayout end sizeAttr table layout off varIdx = case layout of
        [] -> error "SA layout has no ZPL_SIZE (spill blocks are not supported)"
        (anum : rest) ->
            if anum == sizeAttr
            then getU64 end b off
            else
                let l = attrLen table anum
                in if l == 0
                   then walkLayout end sizeAttr table rest
                            (off + getU16 end b (6 + 2 * varIdx)) (varIdx + 1)
                   else walkLayout end sizeAttr table rest (off + l) varIdx

-- ---------------------------------------------------------------------------
-- readPath
-- ---------------------------------------------------------------------------

readPath :: Pool -> (String, String) -> IO (Either String ByteString)
readPath pool dsPath = try (do
    b <- readPath' pool dsPath
    bsLength b `seq` pure b)

readPath' :: Pool -> (String, String) -> IO ByteString
readPath' pool (dsname, relpath) = do
    os <- findDataset pool dsname
    root <- fsRootDir pool os
    let comps = filter (\c -> c /= "") (splitSlash relpath)
    fobj <- resolvePath pool os root comps
    dn <- getDnode (poolCtx pool) os fobj
    size <- fileSize pool os dn
    blocks <- mapM (readBlock (poolCtx pool) dn) [0 .. dnMaxBlkId dn]
    let whole = bsConcatList blocks
    if size > bsLength whole
    then error ("file claims " <> show size <> " bytes but blocks provide only "
                <> show (bsLength whole))
    else pure (bsSub whole 0 size)

splitSlash :: String -> [String]
splitSlash s = go 1 1
  where
    n = strLen s
    go start i =
        if i > n
        then [strSub s start n]
        else if strByte s i == 47                  -- '/'
        then strSub s start (i - 1) : go (i + 1) (i + 1)
        else go start (i + 1)
