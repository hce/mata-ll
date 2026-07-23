-- Lz4: standard LZ4 block decompression, plus the ZFS on-disk framing.
--
-- ZFS stores an LZ4-compressed block as a 4-byte BIG-endian length (the size
-- of the raw LZ4 stream) followed by the stream itself; the physical block is
-- padded out to the allocated size, which is why the explicit length header
-- is needed. The same framing is used for embedded block pointers.
--
-- The block format: a sequence of (token, literals, match) sequences. The
-- token's high nibble is the literal length, low nibble the match length - 4;
-- a nibble of 15 is extended by following bytes (each 255 continues the sum).
-- The match is a 2-byte little-endian distance back into the output already
-- produced; matches may overlap their own output (distance < length), which
-- copies the available slice repeatedly.
--
-- Representation note: output is kept as a flattened `window` ByteString plus
-- a reversed list of chunks appended since the last match. A match forces a
-- flatten (matches read from recent output); literals just push a chunk.
-- Blocks are at most 128K and carry a bounded number of sequences, so the
-- per-match flatten is cheap in practice.

module Lz4 (lz4Decompress, zfsLz4Decompress) where

import LBit (band, shiftR)
import ZBytes (getU32BE)

-- Decompress the ZFS framing: 4-byte big-endian compressed length, then the
-- raw LZ4 block, padded to the physical block size. `dlen` is the expected
-- decompressed (logical) size.
zfsLz4Decompress :: ByteString -> Int -> ByteString
zfsLz4Decompress payload dlen =
    let total = bsLength payload
        clen = getU32BE payload 0
    in if total < 4 || clen < 0 || clen + 4 > total
       then error ("lz4: bad frame: compressed length " <> show clen
                   <> " in a " <> show total <> "-byte payload")
       else lz4Decompress (bsSub payload 4 clen) dlen

-- Decompress a raw LZ4 block to exactly `dlen` bytes (errors otherwise).
lz4Decompress :: ByteString -> Int -> ByteString
lz4Decompress src dlen = go 0 bsEmpty []
  where
    n = bsLength src

    -- ip: input offset; window: flattened output prefix;
    -- pending: chunks appended after window, in reverse order.
    go ip window pending =
        if ip >= n
        then finish window pending
        else
            let tok = bsIndex src ip
                litStep = extend (shiftR tok 4) (ip + 1)
                litlen = fst litStep
                ip1 = snd litStep
                lits = bsSub src ip1 litlen
                ip2 = ip1 + litlen
                pend1 = if litlen == 0 then pending else lits : pending
            in if ip2 >= n
               then finish window pend1
               else matchStep (band tok 15) ip2 window pend1

    matchStep mnib ip window pending =
        let dist = bsGetU16LE src ip
            mStep = extend mnib (ip + 2)
            mlen = fst mStep + 4
            ip1 = snd mStep
            w = flatten window pending
            wlen = bsLength w
            start = wlen - dist
        in if dist <= 0 || start < 0
           then error ("lz4: corrupt stream: match distance " <> show dist
                       <> " with only " <> show wlen <> " bytes produced")
           else go ip1 w [matchBytes w start dist mlen]

    -- Copy `mlen` bytes starting `dist` back; an overlapping match repeats
    -- the available slice.
    matchBytes w start dist mlen =
        if mlen <= dist
        then bsSub w start mlen
        else
            let slice = bsSub w start dist
                reps = div mlen dist + 1
            in bsSub (bsConcatList (replicate reps slice)) 0 mlen

    -- Length-nibble extension: 15 means keep adding bytes, each 255
    -- continuing the run.
    extend nib ip =
        if nib /= 15
        then (nib, ip)
        else ext nib ip
    ext acc ip =
        if ip >= n
        then error "lz4: corrupt stream: truncated length extension"
        else
            let b = bsIndex src ip
            in if b == 255
               then ext (acc + 255) (ip + 1)
               else (acc + b, ip + 1)

    flatten window pending = bsConcatList (window : reverse pending)

    finish window pending =
        let out = flatten window pending
            got = bsLength out
        in if got /= dlen
           then error ("lz4: decompressed to " <> show got
                       <> " bytes, expected " <> show dlen)
           else out
