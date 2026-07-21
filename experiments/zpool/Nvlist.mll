-- Nvlist: parser for the XDR-encoded nvlists found in ZFS vdev labels.
--
-- Layout (all integers big-endian, XDR): a 4-byte header (encoding method,
-- host endianness, two reserved bytes), then the nvlist body: int32 version,
-- uint32 flags, then nvpairs. Each pair is int32 encoded_size, int32
-- decoded_size (both zero terminates the list), the name as an XDR string
-- (uint32 length + bytes, padded to 4), int32 type, int32 element count, and
-- the value. Only the types the pool config actually needs are decoded;
-- anything else is skipped via encoded_size, which counts the whole pair in
-- the XDR stream.

module Nvlist
    ( NvValue(..)
    , parseNvlist
    , nvLookup, nvString, nvU64, nvList
    ) where

import ZBytes (getU32BE, getU64BE)

data NvValue = NvU64 Integer
             | NvString String
             | NvList [(String, NvValue)]
             | NvListArray [[(String, NvValue)]]
             | NvBool
             | NvUnknown Integer
    deriving (Show)

-- DATA_TYPE_* values we decode
tyBoolean :: Integer
tyBoolean = 1

tyUint64 :: Integer
tyUint64 = 8

tyString :: Integer
tyString = 9

tyNvlist :: Integer
tyNvlist = 19

tyNvlistArray :: Integer
tyNvlistArray = 20

-- Parse a label config nvlist, including its 4-byte encoding header.
parseNvlist :: ByteString -> [(String, NvValue)]
parseNvlist b =
    if bsIndex b 0 /= 1
    then error ("nvlist: unsupported encoding " <> show (bsIndex b 0)
                <> " (expected XDR)")
    else fst (parseNvl b 4)

-- An nvlist body: version, flags, pairs. Returns the parsed pairs and the
-- offset just past the terminator.
parseNvl :: ByteString -> Integer -> ([(String, NvValue)], Integer)
parseNvl b off = parsePairs b (off + 8)

parsePairs :: ByteString -> Integer -> ([(String, NvValue)], Integer)
parsePairs b off =
    let esz = getU32BE b off
        dsz = getU32BE b (off + 4)
    in if esz == 0 && dsz == 0
       then ([], off + 8)
       else
           let nameStep = xdrString b (off + 8)
               name = fst nameStep
               o1 = snd nameStep
               typ = getU32BE b o1
               nelem = getU32BE b (o1 + 4)
               valStep = parseValue b (o1 + 8) typ nelem (off + esz)
               restStep = parsePairs b (snd valStep)
           in ((name, fst valStep) : fst restStep, snd restStep)

parseValue :: ByteString -> Integer -> Integer -> Integer -> Integer -> (NvValue, Integer)
parseValue b off typ nelem skipTo =
    if typ == tyUint64
    then (NvU64 (getU64BE b off), off + 8)
    else if typ == tyString
    then let s = xdrString b off in (NvString (fst s), snd s)
    else if typ == tyNvlist
    then let s = parseNvl b off in (NvList (fst s), snd s)
    else if typ == tyNvlistArray
    then let s = parseNvlArray b off nelem in (NvListArray (fst s), snd s)
    else if typ == tyBoolean
    then (NvBool, skipTo)
    else (NvUnknown typ, skipTo)

parseNvlArray :: ByteString -> Integer -> Integer -> ([[(String, NvValue)]], Integer)
parseNvlArray _ off 0 = ([], off)
parseNvlArray b off k =
    let s = parseNvl b off
        rest = parseNvlArray b (snd s) (k - 1)
    in (fst s : fst rest, snd rest)

-- XDR string: uint32 byte length, bytes, padded to a multiple of 4.
xdrString :: ByteString -> Integer -> (String, Integer)
xdrString b off =
    let n = getU32BE b off
        padded = div (n + 3) 4 * 4
    in (bsToString (bsSub b (off + 4) n), off + 4 + padded)

nvLookup :: [(String, NvValue)] -> String -> Maybe NvValue
nvLookup [] _ = Nothing
nvLookup ((k, v) : rest) name =
    if k == name then Just v else nvLookup rest name

-- Accessors that fail with a clear message when the key is missing or has
-- the wrong type — label configs missing these are unusable anyway.
nvString :: [(String, NvValue)] -> String -> String
nvString ps name = case nvLookup ps name of
    Just (NvString s) -> s
    _ -> error ("nvlist: missing string entry " <> name)

nvU64 :: [(String, NvValue)] -> String -> Integer
nvU64 ps name = case nvLookup ps name of
    Just (NvU64 v) -> v
    _ -> error ("nvlist: missing uint64 entry " <> name)

nvList :: [(String, NvValue)] -> String -> [(String, NvValue)]
nvList ps name = case nvLookup ps name of
    Just (NvList l) -> l
    _ -> error ("nvlist: missing nvlist entry " <> name)
