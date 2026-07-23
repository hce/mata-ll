-- Impulse Tracker (.IT) player in MATA-LL
-- Decodes IT modules to raw 16-bit stereo PCM via callback
-- All channel state lives in a single STArray across decode + mix

-- Bitwise FFI
bandB :: Int -> Int -> LuaPure "__mll_band" Int
shrB :: Int -> Int -> LuaPure "__mll_shr" Int

-- Test if bit N is set (0-indexed)
testBit :: Int -> Int -> Bool
testBit x n = bandB (shrB x n) 1 == 1

bsSetByte :: ByteString -> Int -> Int -> ByteString
bsSetByte bs idx val = bsConcat (bsSub bs 0 idx) (bsConcat (bsSingleton val) (bsSub bs (idx + 1) (bsLength bs - idx - 1)))

outRate :: Int
outRate = 44100

clamp :: Int -> Int -> Int -> Int
clamp lo hi x
  | x < lo    = lo
  | x > hi    = hi
  | otherwise = x

-- ========== Module Info (parsed once, threaded read-only) ==========

data ModInfo = ModInfo
    { miFd     :: ByteString
    , miOrdNum :: Int
    , miSpeed  :: Int
    , miTempo  :: Int
    , miNumCh  :: Int
    , miNumSmp :: Int
    }

-- ========== Header ==========

hdrOrdNum :: ByteString -> Int
hdrOrdNum bs = bsGetU16LE bs 32

hdrInsNum :: ByteString -> Int
hdrInsNum bs = bsGetU16LE bs 34

hdrSmpNum :: ByteString -> Int
hdrSmpNum bs = bsGetU16LE bs 36

hdrPatNum :: ByteString -> Int
hdrPatNum bs = bsGetU16LE bs 38

hdrSpeed :: ByteString -> Int
hdrSpeed bs = bsIndex bs 50

hdrTempo :: ByteString -> Int
hdrTempo bs = bsIndex bs 51

getOrder :: ByteString -> Int -> Int
getOrder bs i = bsIndex bs (192 + i)

getChanPan :: ByteString -> Int -> Int
getChanPan bs ch = bsIndex bs (64 + ch)

countActiveChans :: ByteString -> Int -> Int -> Int
countActiveChans bs n i
  | i >= 64            = n
  | getChanPan bs i < 128 = countActiveChans bs (n + 1) (i + 1)
  | otherwise          = countActiveChans bs n (i + 1)

-- ========== Sample Headers ==========

smpOffset :: ByteString -> Int -> Int
smpOffset bs i = bsGetU32LE bs (192 + hdrOrdNum bs + hdrInsNum bs * 4 + i * 4)

smpLen :: ByteString -> Int -> Int
smpLen bs off = bsGetU32LE bs (off + 48)

smpLoopBegin :: ByteString -> Int -> Int
smpLoopBegin bs off = bsGetU32LE bs (off + 52)

smpLoopEnd :: ByteString -> Int -> Int
smpLoopEnd bs off = bsGetU32LE bs (off + 56)

smpC5Freq :: ByteString -> Int -> Int
smpC5Freq bs off = bsGetU32LE bs (off + 60)

smpDataPtr :: ByteString -> Int -> Int
smpDataPtr bs off = bsGetU32LE bs (off + 72)

smpGlobalVol :: ByteString -> Int -> Int
smpGlobalVol bs off = bsIndex bs (off + 17)

smpDefaultVol :: ByteString -> Int -> Int
smpDefaultVol bs off = bsIndex bs (off + 19)

smpFlags :: ByteString -> Int -> Int
smpFlags bs off = bsIndex bs (off + 18)

smpIs16Bit :: Int -> Bool
smpIs16Bit flags = testBit flags 1

smpHasLoop :: Int -> Bool
smpHasLoop flags = testBit flags 4

readSmp :: ByteString -> Int -> Int -> Bool -> Int
readSmp bs dPtr pos is16
  | is16      = bsGetI16LE bs (dPtr + pos * 2)
  | v >= 128  = v - 256
  | otherwise = v
  where v = bsIndex bs (dPtr + pos)

-- ========== Pattern Headers ==========

patOffset :: ByteString -> Int -> Int
patOffset bs i = bsGetU32LE bs (192 + hdrOrdNum bs + hdrInsNum bs * 4 + hdrSmpNum bs * 4 + i * 4)

patRows :: ByteString -> Int -> Int
patRows bs off = bsGetU16LE bs (off + 2)

-- ========== Note Frequency ==========

semiRatio :: Int -> Int
semiRatio 0 = 65536
semiRatio 1 = 69433
semiRatio 2 = 73562
semiRatio 3 = 77936
semiRatio 4 = 82570
semiRatio 5 = 87480
semiRatio 6 = 92682
semiRatio 7 = 98193
semiRatio 8 = 104032
semiRatio 9 = 110218
semiRatio 10 = 116772
semiRatio 11 = 123715
semiRatio _ = 65536

pow2 :: Int -> Int
pow2 0 = 1
pow2 n = 2 * pow2 (n - 1)

noteInc :: Int -> Int -> Int
noteInc note c5 =
    let oct  = (note `div` 12) - 5
        semi = note `mod` 12
        base = (c5 * semiRatio semi * 256) `div` (outRate * 65536)
    in if oct >= 0
       then base * pow2 oct
       else base `div` pow2 (0 - oct)

-- ========== Channel State (STArray) ==========
-- 14 fields per channel packed in a flat array

nf :: Int
nf = 14

fi :: Int -> Int -> Int
fi ch f = ch * nf + f

fiSmp :: Int
fiSmp = 0
fiPos :: Int
fiPos = 1
fi16 :: Int
fi16 = 2
fiInc :: Int
fiInc = 3
fiGVl :: Int
fiGVl = 4
fiVol :: Int
fiVol = 5
fiPan :: Int
fiPan = 6
fiAct :: Int
fiAct = 7
fiLen :: Int
fiLen = 8
fiLpS :: Int
fiLpS = 9
fiLpE :: Int
fiLpE = 10
fiLp :: Int
fiLp = 11
fiDPtr :: Int
fiDPtr = 12
fiC5 :: Int
fiC5 = 13

mkChan :: Int -> [Int]
mkChan pan = [0, 0, 0, 0, 0, 0, pan, 0, 0, 0, 0, 0, 0, 8363]

initChans :: ByteString -> Int -> Int -> [Int]
initChans fd n i
  | i >= n    = []
  | otherwise = mkChan pv ++ initChans fd n (i + 1)
  where
    p  = getChanPan fd i
    pv = if p >= 128 then 32 else p

-- ========== Pattern Decoding (ST monad — O(1) array access) ==========

-- Returns (masks, (lv, (nextOff, jump)))
-- jump: -1 = no jump, >= 0 = Bxx position jump, -2 = Cxx pattern break
decodeRow :: ModInfo -> Int -> STArray s -> ByteString
    -> ByteString
    -> ST s (ByteString, (ByteString, (Int, Int)))
decodeRow mi off arr masks lv =
    decRowLoop mi off arr masks lv (-1)

decRowLoop :: ModInfo -> Int -> STArray s -> ByteString
    -> ByteString -> Int
    -> ST s (ByteString, (ByteString, (Int, Int)))
decRowLoop mi off arr masks lv jump =
    let marker = bsIndex mi.miFd off
    in if marker == 0
       then return (masks, (lv, (off + 1, jump)))
       else let ch   = bandB (marker - 1) 63
                hmb  = testBit marker 7
                off2 = off + 1
                mask = if hmb then bsIndex mi.miFd off2 else bsIndex masks ch
                msk2 = if hmb then bsSetByte masks ch mask else masks
                off3 = if hmb then off2 + 1 else off2
                hasNote = testBit mask 0
                hasIns  = testBit mask 1
                hasVol  = testBit mask 2
                hasCmd  = testBit mask 3
                useLvN  = testBit mask 4
                useLvI  = testBit mask 5
                useLvV  = testBit mask 6
                note = if hasNote then bsIndex mi.miFd off3 else if useLvN then bsIndex lv (ch * 4) else 255
                off4 = if hasNote then off3 + 1 else off3
                ins  = if hasIns then bsIndex mi.miFd off4 else if useLvI then bsIndex lv (ch * 4 + 1) else 0
                off5 = if hasIns then off4 + 1 else off4
                vol  = if hasVol then bsIndex mi.miFd off5 else if useLvV then bsIndex lv (ch * 4 + 2) else 255
                off6 = if hasVol then off5 + 1 else off5
                cmd    = if hasCmd then bsIndex mi.miFd off6 else 0
                cmdVal = if hasCmd then bsIndex mi.miFd (off6 + 1) else 0
                off7 = if hasCmd then off6 + 2 else off6
                lv2 = if hasNote then bsSetByte lv  (ch * 4)     note else lv
                lv3 = if hasIns  then bsSetByte lv2 (ch * 4 + 1) ins  else lv2
                lv4 = if hasVol  then bsSetByte lv3 (ch * 4 + 2) vol  else lv3
                jump2 = if cmd == 2 then cmdVal
                         else if cmd == 3 then -2
                         else jump
            in trigNote mi arr ch note ins vol cmd cmdVal
                >> decRowLoop mi off7 arr msk2 lv4 jump2

trigNote :: ModInfo -> STArray s -> Int -> Int
    -> Int -> Int -> Int -> Int
    -> ST s ()
trigNote mi arr ch note ins vol cmd cmdVal
  | note == 254 = writeSTArray arr (fi ch fiAct) 0
  | otherwise   = do
        when (ins > 0 && ins <= mi.miNumSmp) (loadSmp mi arr ch ins)
        when (note < 120) (setNoteFreq arr ch note)
        applyVol arr ch vol
        applyEffect arr ch cmd cmdVal

applyVol :: STArray s -> Int -> Int -> ST s ()
applyVol arr ch vol
  | vol <= 64              = writeSTArray arr (fi ch fiVol) vol
  | vol >= 128 && vol <= 192 = writeSTArray arr (fi ch fiPan) (vol - 128)
  | otherwise              = return ()

applyEffect :: STArray s -> Int -> Int -> Int -> ST s ()
applyEffect arr ch cmd val
  | cmd == 8                         = writeSTArray arr (fi ch fiPan) (val `div` 4)
  | cmd == 19 && (val `div` 16) == 8 = writeSTArray arr (fi ch fiPan) (((val `mod` 16) * 17) `div` 4)
  | otherwise                        = return ()

setNoteFreq :: STArray s -> Int -> Int -> ST s ()
setNoteFreq arr ch note = do
    c5 <- readSTArray arr (fi ch fiC5)
    let inc = noteInc note c5
    writeSTArray arr (fi ch fiPos) 0
    writeSTArray arr (fi ch fiInc) inc
    writeSTArray arr (fi ch fiAct) 1

loadSmp :: ModInfo -> STArray s -> Int -> Int -> ST s ()
loadSmp mi arr ch sn =
    let off = smpOffset mi.miFd (sn - 1)
        sl  = smpLen mi.miFd off
        lb  = smpLoopBegin mi.miFd off
        le  = smpLoopEnd mi.miFd off
        c5  = smpC5Freq mi.miFd off
        dp  = smpDataPtr mi.miFd off
        dv  = smpDefaultVol mi.miFd off
        gv  = smpGlobalVol mi.miFd off
        fl  = smpFlags mi.miFd off
        hl  = if smpHasLoop fl then 1 else 0
        b16 = if smpIs16Bit fl then 1 else 0
    in do
        writeSTArray arr (fi ch fiSmp) sn
        writeSTArray arr (fi ch fiLen) sl
        writeSTArray arr (fi ch fiLpS) lb
        writeSTArray arr (fi ch fiLpE) le
        writeSTArray arr (fi ch fiLp) hl
        writeSTArray arr (fi ch fiDPtr) dp
        writeSTArray arr (fi ch fiC5) c5
        writeSTArray arr (fi ch fiVol) dv
        writeSTArray arr (fi ch fi16) b16
        writeSTArray arr (fi ch fiGVl) gv

-- ========== Mixing (ST monad — same STArray as decoding) ==========

mixTick :: ModInfo -> STArray s -> Int
    -> [ByteString] -> ST s [ByteString]
mixTick mi arr spt chunks = do
    pcm <- mixFrames mi arr spt []
    return (pcm : chunks)

mixFrames :: ModInfo -> STArray s -> Int
    -> [ByteString] -> ST s ByteString
-- The accumulated per-frame chunks are concatenated STRICTLY (`seq`, GHC's
-- `return $!`): `return` is non-strict, so without the force the concat stays
-- a thunk retaining every per-frame cons cell until the chunk is finally
-- written -- the classic lazy-accumulator space leak (GHC behaves the same).
-- Forcing here collapses each tick to one compact string as it is produced.
mixFrames mi arr 0 acc =
    let pcm = bsConcatList (reverse acc)
    in pcm `seq` return pcm
mixFrames mi arr n acc = do
    (l, r) <- mixFrame mi arr 0 0 0
    let ml  = (l * 48) `div` (128 * 3)
    let mr  = (r * 48) `div` (128 * 3)
    let pcm = bsConcat (bsPutI16LE (clamp (0 - 32768) 32767 ml)) (bsPutI16LE (clamp (0 - 32768) 32767 mr))
    mixFrames mi arr (n - 1) (pcm : acc)

mixFrame :: ModInfo -> STArray s -> Int
    -> Int -> Int -> ST s (Int, Int)
mixFrame mi arr ch la ra
  | ch >= mi.miNumCh = return (la, ra)
  | otherwise = do
        act <- readSTArray arr (fi ch fiAct)
        if act == 0
        then mixFrame mi arr (ch + 1) la ra
        else do
            pos  <- readSTArray arr (fi ch fiPos)
            sl   <- readSTArray arr (fi ch fiLen)
            dp   <- readSTArray arr (fi ch fiDPtr)
            vol  <- readSTArray arr (fi ch fiVol)
            pan  <- readSTArray arr (fi ch fiPan)
            is16 <- readSTArray arr (fi ch fi16)
            gvl  <- readSTArray arr (fi ch fiGVl)
            let smpPos = pos `div` 256
            let smp = if smpPos < sl then readSmp mi.miFd dp smpPos (is16 == 1) else 0
            let sv  = if is16 == 1 then (smp * vol * gvl * 128) `div` (64 * 64 * 128) else (smp * vol * gvl * 128 * 256) `div` (64 * 64 * 128)
            let nl = la + (sv * (64 - pan)) `div` 64
            let nr = ra + (sv * pan) `div` 64
            advPos arr ch
            mixFrame mi arr (ch + 1) nl nr

advPos :: STArray s -> Int -> ST s ()
advPos arr ch = do
    pos <- readSTArray arr (fi ch fiPos)
    inc <- readSTArray arr (fi ch fiInc)
    sl  <- readSTArray arr (fi ch fiLen)
    hl  <- readSTArray arr (fi ch fiLp)
    ls  <- readSTArray arr (fi ch fiLpS)
    le  <- readSTArray arr (fi ch fiLpE)
    let nPos = pos + inc
    let slFP = sl * 256
    let lsFP = ls * 256
    let leFP = le * 256
    let fPos = if hl == 1 && nPos >= leFP && leFP > lsFP then lsFP + ((nPos - lsFP) `mod` (leFP - lsFP)) else nPos
    writeSTArray arr (fi ch fiPos) fPos
    when (hl == 0 && nPos >= slFP) (writeSTArray arr (fi ch fiAct) 0)

-- ========== Inner loop: decode + mix one pattern (pure, inside runST) ==========

doTicks :: ModInfo -> STArray s -> Int
    -> [ByteString] -> ST s [ByteString]
doTicks mi arr spt chunks =
    doTickLoop mi arr spt 0 chunks

doTickLoop :: ModInfo -> STArray s -> Int
    -> Int -> [ByteString] -> ST s [ByteString]
doTickLoop mi arr spt tick chunks =
    if tick >= mi.miSpeed
    then return chunks
    else do
        chunks2 <- mixTick mi arr spt chunks
        doTickLoop mi arr spt (tick + 1) chunks2

-- Returns (chunks, (state, jump)) where jump is -1 for no jump
doRows :: ModInfo -> STArray s -> ByteString -> ByteString
    -> Int -> Int -> Int
    -> [ByteString] -> ST s ([ByteString], ([Int], Int))
doRows mi arr masks lv dataOff row numRows chunks
  | row >= numRows = do
        st2 <- stArrayToList arr
        return (chunks, (st2, -1))
  | otherwise = do
        (masks2, (lv2, (nextOff, jump))) <- decodeRow mi dataOff arr masks lv
        let spt     = (outRate * 60) `div` (mi.miTempo * 24)
        chunks2 <- doTicks mi arr spt chunks
        if jump >= 0 || jump == (-2)
        then do
            st2 <- stArrayToList arr
            return (chunks2, (st2, jump))
        else doRows mi arr masks2 lv2 nextOff (row + 1) numRows chunks2

-- Process one pattern: enter runST, decode all rows + mix
-- Returns (chunks, (state, jump))
processPattern :: ModInfo -> [Int] -> Int -> Int
    -> ([ByteString], ([Int], Int))
processPattern mi st pOff nRows =
    let masks = bsReplicate 64 0
        lv    = bsReplicate 256 0
    in runST (do
        arr <- newSTArrayFromList st
        doRows mi arr masks lv (pOff + 8) 0 nRows [])

-- ========== Playback Loop (LuaIO for output callback) ==========

emitChunks :: (ByteString -> LuaIO s ()) -> [ByteString] -> LuaIO s ()
emitChunks sw [] = return ()
emitChunks sw (c:cs) = sw c >> emitChunks sw cs

findNextPos :: [Int] -> Int -> Int -> Maybe Int
findNextPos playedPositions maxPosition n
    | n < maxPosition = if n `elem` playedPositions
                        then findNextPos playedPositions maxPosition (n + 1)
                        else Just n
    | otherwise       = Nothing

handleEnd :: ModInfo -> (ByteString -> LuaIO s ()) -> [Int]
    -> Bool -> [Int] -> LuaIO s ()
handleEnd mi sw st noLoop playedPositions
  | noLoop    = case findNextPos playedPositions mi.miOrdNum 0 of
        Nothing -> return ()
        Just newPos -> doOrders mi sw st newPos noLoop (newPos:playedPositions)
  | otherwise = return ()

doOrders :: ModInfo -> (ByteString -> LuaIO s ()) -> [Int]
    -> Int -> Bool -> [Int] -> LuaIO s ()
doOrders mi sw st idx noLoop playedPositions
  | idx >= mi.miOrdNum = return ()
  | pat == 254         = doOrders mi sw st (idx + 1) noLoop (idx:playedPositions)
  | pat == 255         = handleEnd mi sw st noLoop playedPositions
  | otherwise          =
        case processPattern mi st pOff nRows of
            (chunks, (st2, jump)) ->
                let nextIdx = if jump >= 0 then jump else idx + 1
                    played2 = idx:playedPositions
                in emitChunks sw (reverse chunks)
                    >> if jump >= 0 && noLoop && jump `elem` played2
                       then handleEnd mi sw st2 noLoop played2
                       else doOrders mi sw st2 nextIdx noLoop played2
  where
    pat   = getOrder mi.miFd idx
    pOff  = patOffset mi.miFd pat
    nRows = patRows mi.miFd pOff

-- Find IMPM magic to skip UMX/container headers.
-- Returns the offset of 'I' in 'IMPM', or 0 if the file starts with it.
findIMPM :: ByteString -> Int -> Int
findIMPM bs i
  | i + 3 >= bsLength bs = 0
  | bsIndex bs i == 73 && bsIndex bs (i + 1) == 77 && bsIndex bs (i + 2) == 80 && bsIndex bs (i + 3) == 77 = i
  | otherwise = findIMPM bs (i + 1)

export play :: (ByteString -> LuaIO s ()) -> ByteString -> Bool -> LuaIO s ()
play swallower fd noLoop =
    (liftIO $ putStrLn "Pure mata-ll Impulse Tracker decoder") >>
    let offset = findIMPM fd 0
        itData = if offset == 0 then fd else bsSub fd offset (bsLength fd - offset)
        numCh  = countActiveChans itData 0 0
        st     = initChans itData numCh 0
        mi     = ModInfo itData (hdrOrdNum itData) (hdrSpeed itData) (hdrTempo itData) numCh (hdrSmpNum itData)
    in doOrders mi swallower st 0 noLoop []

main :: IO ()
main = putStrLn "ImpulseTracker player written in mata-ll (https://matall.org)" >>
    putStrLn "This is a Lua/mata-ll interop via callbacks example; please" >>
    putStrLn "Invoke via ctracker.lua from the same directory instead."
