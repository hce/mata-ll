-- Ed25519 digital signatures (RFC 8032)
-- Pure MATA-LL implementation, LuaJIT-compatible (53-bit safe)
--
-- Field arithmetic: TweetNaCl-style 16 x 16-bit limbs
-- SHA-512: 32-bit half-word pairs for 64-bit portability

import Data.List (drop, replicate, take, foldl')

-- ================================================================
-- FFI bindings
-- ================================================================

xorB :: Integer -> Integer -> LuaPure "__mll_bxor" Integer
bandB :: Integer -> Integer -> LuaPure "__mll_band" Integer
borB :: Integer -> Integer -> LuaPure "__mll_bor" Integer
shlB :: Integer -> Integer -> LuaPure "__mll_shl" Integer
shrB :: Integer -> Integer -> LuaPure "__mll_shr" Integer
strChar :: Integer -> LuaPure "string.char" String
strByte :: String -> Integer -> LuaPure "string.byte" Integer
strLen :: String -> LuaPure "string.len" Integer

-- ================================================================
-- Utilities
-- ================================================================

u32 :: Integer -> Integer
u32 x = bandB x 4294967295

idx :: [a] -> Integer -> a
idx (x:_)  0 = x
idx (_:xs) n = idx xs (n - 1)
idx _      _ = error "idx: out of bounds"

listSet :: [a] -> Integer -> a -> [a]
listSet (_:xs) 0 v = v : xs
listSet (x:xs) n v = x : listSet xs (n - 1) v
listSet xs     _ _ = xs

-- ================================================================
-- Type aliases
-- ================================================================

type W64 = (Integer, Integer)
-- GF = [Integer] (16-element list, each limb ~16 bits)

-- ================================================================
-- 64-bit word operations (hi, lo) pairs of 32-bit values
-- ================================================================

w64 :: Integer -> Integer -> W64
w64 h l = (h, l)

w64Xor :: W64 -> W64 -> W64
w64Xor (ah, al) (bh, bl) = (xorB ah bh, xorB al bl)

w64And :: W64 -> W64 -> W64
w64And (ah, al) (bh, bl) = (bandB ah bh, bandB al bl)

w64Not :: W64 -> W64
w64Not (h, l) = (xorB h 4294967295, xorB l 4294967295)

w64Add :: W64 -> W64 -> W64
w64Add (ah, al) (bh, bl) =
    let lo = bandB (borB al 0 + borB bl 0) 4294967295
        hi = bandB (borB ah 0 + borB bh 0 + shrB (borB al 0 + borB bl 0) 32) 4294967295
    in (hi, lo)

w64Add4 :: W64 -> W64 -> W64 -> W64 -> W64
w64Add4 a b c d = w64Add (w64Add a b) (w64Add c d)

-- Right rotate by n bits (0 < n < 32)
w64RotRSmall :: W64 -> Integer -> W64
w64RotRSmall (h, l) n =
    (u32 (borB (shrB h n) (shlB l (32 - n))),
     u32 (borB (shrB l n) (shlB h (32 - n))))

-- Right rotate by n bits (32 <= n < 64)
w64RotRBig :: W64 -> Integer -> W64
w64RotRBig (h, l) n = w64RotRSmall (l, h) (n - 32)

-- Swap halves
w64Swap :: W64 -> W64
w64Swap (h, l) = (l, h)

-- General right rotate
w64RotR :: W64 -> Integer -> W64
w64RotR w n
    | n == 0    = w
    | n < 32    = w64RotRSmall w n
    | n == 32   = w64Swap w
    | otherwise = w64RotRBig w n

-- Right shift (logical)
w64ShrR :: W64 -> Integer -> W64
w64ShrR (h, l) n
    | n == 0    = (h, l)
    | n < 32    = (shrB h n, u32 (borB (shrB l n) (shlB h (32 - n))))
    | n == 32   = (0, h)
    | otherwise = (0, shrB h (n - 32))

-- ================================================================
-- SHA-512 round constants
-- ================================================================

sha512K0 :: [W64]
sha512K0 = [(1116352408, 3609767458), (1899447441, 602891725), (3049323471, 3964484399), (3921009573, 2173295548), (961987163, 4081628472), (1508970993, 3053834265), (2453635748, 2937671579), (2870763221, 3664609560), (3624381080, 2734883394), (310598401, 1164996542), (607225278, 1323610764), (1426881987, 3590304994), (1925078388, 4068182383), (2162078206, 991336113), (2614888103, 633803317), (3248222580, 3479774868), (3835390401, 2666613458), (4022224774, 944711139), (264347078, 2341262773), (604807628, 2007800933)]
sha512K1 :: [W64]
sha512K1 = [(770255983, 1495990901), (1249150122, 1856431235), (1555081692, 3175218132), (1996064986, 2198950837), (2554220882, 3999719339), (2821834349, 766784016), (2952996808, 2566594879), (3210313671, 3203337956), (3336571891, 1034457026), (3584528711, 2466948901), (113926993, 3758326383), (338241895, 168717936), (666307205, 1188179964), (773529912, 1546045734), (1294757372, 1522805485), (1396182291, 2643833823), (1695183700, 2343527390), (1986661051, 1014477480), (2177026350, 1206759142), (2456956037, 344077627)]
sha512K2 :: [W64]
sha512K2 = [(2730485921, 1290863460), (2820302411, 3158454273), (3259730800, 3505952657), (3345764771, 106217008), (3516065817, 3606008344), (3600352804, 1432725776), (4094571909, 1467031594), (275423344, 851169720), (430227734, 3100823752), (506948616, 1363258195), (659060556, 3750685593), (883997877, 3785050280), (958139571, 3318307427), (1322822218, 3812723403), (1537002063, 2003034995), (1747873779, 3602036899), (1955562222, 1575990012), (2024104815, 1125592928), (2227730452, 2716904306), (2361852424, 442776044)]
sha512K3 :: [W64]
sha512K3 = [(2428436474, 593698344), (2756734187, 3733110249), (3204031479, 2999351573), (3329325298, 3815920427), (3391569614, 3928383900), (3515267271, 566280711), (3940187606, 3454069534), (4118630271, 4000239992), (116418474, 1914138554), (174292421, 2731055270), (289380356, 3203993006), (460393269, 320620315), (685471733, 587496836), (852142971, 1086792851), (1017036298, 365543100), (1126000580, 2618297676), (1288033470, 3409855158), (1501505948, 4234509866), (1607167915, 987167468), (1816402316, 1246189591)]
sha512K :: [W64]
sha512K = sha512K0 ++ sha512K1 ++ sha512K2 ++ sha512K3

-- ================================================================
-- SHA-512 functions
-- ================================================================

sha512Ch :: W64 -> W64 -> W64 -> W64
sha512Ch x y z = w64Xor (w64And x y) (w64And (w64Not x) z)

sha512Maj :: W64 -> W64 -> W64 -> W64
sha512Maj x y z = w64Xor (w64And x y) (w64Xor (w64And x z) (w64And y z))

sha512BSig0 :: W64 -> W64
sha512BSig0 x = w64Xor (w64RotR x 28) (w64Xor (w64RotR x 34) (w64RotR x 39))

sha512BSig1 :: W64 -> W64
sha512BSig1 x = w64Xor (w64RotR x 14) (w64Xor (w64RotR x 18) (w64RotR x 41))

sha512SSig0 :: W64 -> W64
sha512SSig0 x = w64Xor (w64RotR x 1) (w64Xor (w64RotR x 8) (w64ShrR x 7))

sha512SSig1 :: W64 -> W64
sha512SSig1 x = w64Xor (w64RotR x 19) (w64Xor (w64RotR x 61) (w64ShrR x 6))

-- Parse 8 bytes (big-endian) into W64
bytesToW64 :: [Integer] -> W64
bytesToW64 bs =
    let b0 = idx bs 0
        b1 = idx bs 1
        b2 = idx bs 2
        b3 = idx bs 3
        b4 = idx bs 4
        b5 = idx bs 5
        b6 = idx bs 6
        b7 = idx bs 7
        hi = u32 (borB (borB (shlB b0 24) (shlB b1 16)) (borB (shlB b2 8) b3))
        lo = u32 (borB (borB (shlB b4 24) (shlB b5 16)) (borB (shlB b6 8) b7))
    in (hi, lo)

-- Serialize W64 to 8 bytes (big-endian)
w64ToBytes :: W64 -> [Integer]
w64ToBytes (h, l) = [bandB (shrB h 24) 255, bandB (shrB h 16) 255, bandB (shrB h 8) 255, bandB h 255, bandB (shrB l 24) 255, bandB (shrB l 16) 255, bandB (shrB l 8) 255, bandB l 255]

-- Parse bytes into list of W64 (16 words per 128-byte block)
bytesToW64s :: [Integer] -> [W64]
bytesToW64s [] = []
bytesToW64s bs = bytesToW64 (take 8 bs) : bytesToW64s (drop 8 bs)

-- SHA-512 message schedule: expand 16 words to 80
sha512Schedule :: [W64] -> [W64]
sha512Schedule ws = schedGo ws 16
  where
    schedGo acc 80 = acc
    schedGo acc i =
        let w = w64Add4 (sha512SSig1 (idx acc (i - 2)))
                        (idx acc (i - 7))
                        (sha512SSig0 (idx acc (i - 15)))
                        (idx acc (i - 16))
        in schedGo (acc ++ [w]) (i + 1)

-- SHA-512 initial hash values
sha512IV :: [W64]
sha512IV = [(1779033703, 4089235720), (3144134277, 2227873595), (1013904242, 4271175723), (2773480762, 1595750129), (1359893119, 2917565137), (2600822924, 725511199), (528734635, 4215389547), (1541459225, 327033209)]

-- One SHA-512 compression round
sha512Round :: [W64] -> [W64] -> Integer -> [W64]
sha512Round state schedule i =
    let a = idx state 0
        b = idx state 1
        c = idx state 2
        d = idx state 3
        e = idx state 4
        f = idx state 5
        g = idx state 6
        h = idx state 7
        t1 = w64Add4 h (sha512BSig1 e) (sha512Ch e f g) (w64Add (idx schedule i) (idx sha512K i))
        t2 = w64Add (sha512BSig0 a) (sha512Maj a b c)
    in [w64Add t1 t2, a, b, c, w64Add d t1, e, f, g]

-- Compress one 128-byte block
sha512Compress :: [W64] -> [W64] -> [W64]
sha512Compress state block =
    let schedule = sha512Schedule block
        final = compressGo state schedule 0
    in zipWith w64Add state final
  where
    compressGo st _ 80 = st
    compressGo st sched i = compressGo (sha512Round st sched i) sched (i + 1)

-- SHA-512 padding: append 1 bit, zeros, 128-bit length (big-endian)
sha512Pad :: [Integer] -> [[Integer]]
sha512Pad msg = blocks (msg ++ [128] ++ replicate padLen 0 ++ lenBytes)
  where
    len = length msg
    -- pad to multiple of 128 bytes, with 16 bytes for length
    padLen = (111 - len) `mod` 128
    -- length in bits as 16 bytes big-endian (only low 8 bytes matter for < 2^53)
    bitLen = len * 8
    lenBytes = [0, 0, 0, 0, 0, 0, 0, 0, bandB (shrB bitLen 56) 255, bandB (shrB bitLen 48) 255, bandB (shrB bitLen 40) 255, bandB (shrB bitLen 32) 255, bandB (shrB bitLen 24) 255, bandB (shrB bitLen 16) 255, bandB (shrB bitLen 8) 255, bandB bitLen 255]
    blocks [] = []
    blocks xs = take 128 xs : blocks (drop 128 xs)

-- Full SHA-512 hash: ByteString -> ByteString
sha512 :: ByteString -> ByteString
sha512 input =
    let msg = bsUnpack input
        padded = sha512Pad msg
        wordBlocks = map bytesToW64s padded
        finalState = foldl' sha512Compress sha512IV wordBlocks
    in bsPack (concatMap w64ToBytes finalState)

-- ================================================================
-- Field25519 arithmetic (mod p = 2^255 - 19)
-- 16 limbs, each nominally 16 bits
-- ================================================================

gfZero :: [Integer]
gfZero = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]

gfOne :: [Integer]
gfOne = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]

gfAdd :: [Integer] -> [Integer] -> [Integer]
gfAdd = zipWith (+)

gfSub :: [Integer] -> [Integer] -> [Integer]
gfSub = zipWith (-)

-- Carry propagation (handles negative limbs via floor division)
gfCarry :: [Integer] -> [Integer]
gfCarry xs = carryPass (carryPass xs)

-- Single carry pass: propagate through all limbs, wrap top carry via *38
carryPass :: [Integer] -> [Integer]
carryPass xs = cpWrap (cpLinear xs 0)

-- Linear carry propagation (no wrapping)
cpLinear :: [Integer] -> Integer -> ([Integer], Integer)
cpLinear [] carry = ([], carry)
cpLinear (x:rest) carry =
    let val = x + carry
        c = val `div` 65536
        lo = val - c * 65536
    in case cpLinear rest c of
        (rest', finalCarry) -> (lo : rest', finalCarry)

-- Wrap top carry back to limb 0 via *38, then repropagate
cpWrap :: ([Integer], Integer) -> [Integer]
cpWrap (limbs, topCarry) = case limbs of
    (l0:rest) -> cpRecarry ((l0 + topCarry * 38) : rest) 0
    _ -> limbs

-- Repropagate carries after wrap (most limbs won't need it)
cpRecarry :: [Integer] -> Integer -> [Integer]
cpRecarry [] _ = []
cpRecarry (x:rest) carry =
    let val = x + carry
        c = val `div` 65536
        lo = val - c * 65536
    in lo : cpRecarry rest c

-- Field multiplication: output[o] = sum(a[i]*b[o-i]) + 38*sum(a[i]*b[o+16-i])
gfMul :: [Integer] -> [Integer] -> [Integer]
gfMul a b = gfCarry (map (mulLimb a b) [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15])

mulLimb :: [Integer] -> [Integer] -> Integer -> Integer
mulLimb a b o = mlDirect a b o 0 + 38 * mlWrap a b o (o + 1)

mlDirect :: [Integer] -> [Integer] -> Integer -> Integer -> Integer
mlDirect a b o j
  | j > o    = 0
  | j == o   = idx a j * idx b 0
  | otherwise = idx a j * idx b (o - j) + mlDirect a b o (j + 1)

mlWrap :: [Integer] -> [Integer] -> Integer -> Integer -> Integer
mlWrap a b o j
  | j > 15   = 0
  | j == 15  = idx a 15 * idx b (o + 1)
  | otherwise = idx a j * idx b (o + 16 - j) + mlWrap a b o (j + 1)

-- Field squaring
gfSqr :: [Integer] -> [Integer]
gfSqr a = gfMul a a

-- Conditional swap (constant-time-ish): if b == 1 swap, else don't
gfSel :: Integer -> [Integer] -> [Integer] -> [Integer]
gfSel 0 p _ = p
gfSel _ _ q = q

-- Field inversion: a^(p-2) mod p via addition chain
-- p-2 = 2^255 - 21
gfInv :: [Integer] -> [Integer]
gfInv a = gfInvChain a

gfInvChain :: [Integer] -> [Integer]
gfInvChain z =
    let t0 = gfSqr z         -- z^2
        t1 = gfSqr t0        -- z^4
        t1b = gfSqr t1       -- z^8
        t1c = gfMul t1b z    -- z^9
        t0b = gfMul t0 t1c   -- z^11
        t0c = gfSqr t0b      -- z^22
        t0d = gfMul t0c t1c  -- z^(2^5 - 1) = z^31
        -- z^(2^10 - 1)
        t1d = sqrN t0d 5
        t1e = gfMul t1d t0d
        -- z^(2^20 - 1)
        t2 = sqrN t1e 10
        t2b = gfMul t2 t1e
        -- z^(2^40 - 1)
        t2c = sqrN t2b 20
        t2d = gfMul t2c t2b
        -- z^(2^50 - 1)
        t2e = sqrN t2d 10
        t2f = gfMul t2e t1e
        -- z^(2^100 - 1)
        t3 = sqrN t2f 50
        t3b = gfMul t3 t2f
        -- z^(2^200 - 1)
        t3c = sqrN t3b 100
        t3d = gfMul t3c t3b
        -- z^(2^250 - 1)
        t3e = sqrN t3d 50
        t3f = gfMul t3e t2f
        -- z^(2^255 - 21)
        t4 = sqrN t3f 5
    in gfMul t4 t0b

sqrN :: [Integer] -> Integer -> [Integer]
sqrN x 0 = x
sqrN x n = sqrN (gfSqr x) (n - 1)

-- z^((p+3)/8) = z^(2^252 - 3) used for sqrt in point decompression
pow2523 :: [Integer] -> [Integer]
pow2523 z =
    let t0 = gfSqr z
        t1 = gfSqr t0
        t1b = gfSqr t1
        t1c = gfMul t1b z
        t0b = gfMul t0 t1c
        t0c = gfSqr t0b
        t0d = gfMul t0c t1c
        t1d = sqrN t0d 5
        t1e = gfMul t1d t0d
        t2 = sqrN t1e 10
        t2b = gfMul t2 t1e
        t2c = sqrN t2b 20
        t2d = gfMul t2c t2b
        t2e = sqrN t2d 10
        t2f = gfMul t2e t1e
        t3 = sqrN t2f 50
        t3b = gfMul t3 t2f
        t3c = sqrN t3b 100
        t3d = gfMul t3c t3b
        t3e = sqrN t3d 50
        t3f = gfMul t3e t2f
        t4 = sqrN t3f 2
    in gfMul t4 z

-- p as limbs (2^255 - 19)
gfP :: [Integer]
gfP = [65517, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 65535, 32767]

-- Try subtracting p; if result is negative (top bit set after carry), keep original
gfCondSub :: [Integer] -> [Integer]
gfCondSub t =
    let m = gfCarry (gfSub t gfP)
        topBit = shrB (idx m 15) 15
    in if topBit == 1 then t else m

-- Pack GF to 32 bytes (little-endian), fully reduced mod p
gfPack :: [Integer] -> [Integer]
gfPack a =
    let t0 = gfCarry (gfCarry a)
        t1 = gfCondSub t0
        t2 = gfCondSub t1
    in packLimbs t2

packLimbs :: [Integer] -> [Integer]
packLimbs ls = plGo ls 0
  where
    plGo _ 16 = []
    plGo xs i = let v = idx xs i
                in bandB v 255 : bandB (shrB v 8) 255 : plGo xs (i + 1)

-- Unpack 32 bytes to GF (little-endian, 16-bit limbs)
gfUnpack :: [Integer] -> [Integer]
gfUnpack bs = guGo 0
  where
    guGo 16 = []
    guGo i = let lo = idx bs (i * 2)
                 hi = idx bs (i * 2 + 1)
             in borB lo (shlB hi 8) : guGo (i + 1)

-- Curve constant d = -121665/121666 mod p
gfD :: [Integer]
gfD = [30883, 4953, 19914, 30187, 55467, 16705, 2637, 112,
       59544, 30585, 16505, 36039, 65139, 11119, 27886, 20995]

-- 2*d
gfD2 :: [Integer]
gfD2 = [61785, 9906, 39828, 60374, 45398, 33411, 5274, 224,
        53552, 61171, 33010, 6542, 64743, 22239, 55772, 9222]

-- sqrt(-1) mod p
gfI :: [Integer]
gfI = [41136, 18958, 6951, 50414, 58488, 44335, 6150, 12099,
       55207, 15867, 153, 11085, 57099, 20417, 9344, 11139]

-- ================================================================
-- Ed25519 point operations (extended twisted Edwards)
-- Point = (X, Y, Z, T) where x=X/Z, y=Y/Z, x*y=T/Z
-- ================================================================

data ExtPoint = ExtPoint [Integer] [Integer] [Integer] [Integer]

pointIdentity :: ExtPoint
pointIdentity = ExtPoint gfZero gfOne gfOne gfZero

-- Point addition (unified formula)
pointAdd :: ExtPoint -> ExtPoint -> ExtPoint
pointAdd (ExtPoint x1 y1 z1 t1) (ExtPoint x2 y2 z2 t2) =
    let a = gfMul (gfSub y1 x1) (gfSub y2 x2)
        b = gfMul (gfAdd y1 x1) (gfAdd y2 x2)
        c = gfMul (gfMul t1 t2) gfD2
        d = gfMul z1 (gfAdd z2 z2)
        e = gfSub b a
        f = gfSub d c
        g = gfAdd d c
        h = gfAdd b a
        x3 = gfMul e f
        y3 = gfMul g h
        t3 = gfMul e h
        z3 = gfMul f g
    in ExtPoint x3 y3 z3 t3

-- Point doubling
pointDouble :: ExtPoint -> ExtPoint
pointDouble (ExtPoint x1 y1 z1 _) =
    let a = gfSqr x1
        b = gfSqr y1
        c = gfAdd (gfSqr z1) (gfSqr z1)
        h = gfAdd a b
        e = gfSub h (gfSqr (gfAdd x1 y1))
        g = gfSub a b
        f = gfAdd c g
        x3 = gfMul e f
        y3 = gfMul g h
        t3 = gfMul e h
        z3 = gfMul f g
    in ExtPoint x3 y3 z3 t3

-- Negate a point
pointNeg :: ExtPoint -> ExtPoint
pointNeg (ExtPoint x y z t) = ExtPoint (gfSub gfZero x) y z (gfSub gfZero t)

-- Scalar multiplication: double-and-add, scanning bits 254 -> 0
scalarMult :: [Integer] -> ExtPoint -> ExtPoint
scalarMult scalar p = smGo 254 pointIdentity
  where
    smGo (-1) acc = acc
    smGo bit acc =
        let doubled = pointDouble acc
            byteIdx = bit `div` 8
            bitIdx = bit `mod` 8
            bitVal = bandB (shrB (idx scalar byteIdx) bitIdx) 1
        in smGo (bit - 1) (if bitVal == 1 then pointAdd doubled p else doubled)

-- Base point
basePoint :: ExtPoint
basePoint = ExtPoint bpX bpY gfOne (gfMul bpX bpY)
  where
    bpX = [54554, 36645, 11616, 51542, 42930, 38181, 51040, 26924,
           56412, 64982, 57905, 49316, 21502, 52590, 14035, 8553]
    bpY = [26200, 26214, 26214, 26214, 26214, 26214, 26214, 26214,
           26214, 26214, 26214, 26214, 26214, 26214, 26214, 26214]

-- Encode point to 32 bytes (y with sign bit of x in top bit)
pointEncode :: ExtPoint -> ByteString
pointEncode (ExtPoint x y z _) =
    let zi = gfInv z
        xr = gfMul x zi
        yr = gfMul y zi
        bs = gfPack yr
        -- Set high bit of byte 31 to low bit of xr
        xBit = bandB (idx (gfPack xr) 0) 1
        b31 = borB (idx bs 31) (shlB xBit 7)
    in bsPack (take 31 bs ++ [b31])

-- Decode 32 bytes to point (recover x from y)
pointDecode :: ByteString -> ExtPoint
pointDecode bs =
    let raw = bsUnpack bs
        -- Extract sign bit from top of byte 31
        xSign = shrB (idx raw 31) 7
        -- Clear sign bit for y
        yBytes = take 31 raw ++ [bandB (idx raw 31) 127]
        y = gfUnpack yBytes
        -- x^2 = (y^2 - 1) / (d*y^2 + 1)
        y2 = gfSqr y
        x2num = gfSub y2 gfOne
        x2den = gfAdd (gfMul gfD y2) gfOne
        x2 = gfMul x2num (gfInv x2den)
        -- x = x2^((p+3)/8) mod p
        x0 = pow2523 x2
        -- Check: if x0^2 != x2, multiply by sqrt(-1)
        check = gfSub (gfSqr x0) x2
        x1 = if gfIsZero (gfCarry check) then x0 else gfMul x0 gfI
        -- Adjust sign
        xPacked = gfPack x1
        needFlip = bandB (idx xPacked 0) 1
        x2final = if needFlip /= xSign then gfSub gfZero x1 else x1
        t = gfMul x2final y
    in ExtPoint x2final y gfOne t

-- Check if a field element is zero (after carry)
gfIsZero :: [Integer] -> Bool
gfIsZero xs = gfizGo xs
  where
    gfizGo [] = True
    gfizGo (0:rest) = gfizGo rest
    gfizGo _ = False

-- ================================================================
-- Scalar arithmetic mod L
-- L = 2^252 + 27742317777372353535851937790883648493
-- ================================================================

-- L as bytes (little-endian)
scL :: [Integer]
scL = [237, 211, 245, 92, 26, 99, 18, 88, 214, 156, 247, 162, 222, 249, 222, 20,
       0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 16]

-- Reduce 64-byte little-endian integer mod L (TweetNaCl's modL)
scReduce :: [Integer] -> [Integer]
scReduce x = srFinish (srGo (map borB0 x) 63)
  where
    borB0 v = borB v 0
    srGo xs i
      | i < 32 = xs
      | otherwise =
          let carry = idx xs i
              xs1 = listSet xs i 0
          in srGo (srSubMul xs1 carry (i - 32)) (i - 1)

    srSubMul xs carry off = ssmGo xs carry off 0

    ssmGo xs carry off 32 =
        let v = idx xs (off + 32) + carry
        in listSet xs (off + 32) v
    ssmGo xs carry off j =
        let v = idx xs (off + j) - carry * idx scL j
            lo = bandB v 255
            borrow = 0 - ((v - lo) `div` 256)
        in ssmGo (listSet xs (off + j) lo) borrow off (j + 1)

    srFinish xs = srFinish2 (srCarry xs 0)

    srCarry xs 32 = xs
    srCarry xs i =
        let v = idx xs i
            c = v `div` 256
            lo = v - c * 256
        in srCarry (listSet (listSet xs i lo) (i + 1) (idx xs (i + 1) + c)) (i + 1)

    srFinish2 xs =
        let borrow = srSubL xs 0 0
        in srApplyBorrow xs borrow 0

    srSubL xs borrow 32 = borrow
    srSubL xs borrow i =
        let v = idx xs i - idx scL i + borrow
        in if v < 0 then srSubL xs (-1) (i + 1) else srSubL xs 0 (i + 1)

    srApplyBorrow xs borrow i
      | i >= 32 = take 32 xs
      | otherwise =
          let v = idx xs i + borrow * idx scL i
              lo = bandB v 255
              c = (v - lo) `div` 256
          in srApplyBorrow (listSet xs i lo) c (i + 1)

-- Scalar multiply-add: (a * b + c) mod L
-- a, b, c are 32-byte scalars; result is 32 bytes
scMulAdd :: [Integer] -> [Integer] -> [Integer] -> [Integer]
scMulAdd a b c = scReduce product
  where
    product = foldl' addMulByte (replicate 64 0 `addBytes` c) allPairs
    allPairs = [(i, j) | i <- [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
                               16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
                         j <- [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
                               16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31]]
    addMulByte acc (i, j) =
        let v = idx acc (i + j) + idx a i * idx b j
        in listSet acc (i + j) v
    addBytes xs ys = abGo xs ys 0
    abGo xs [] _ = xs
    abGo xs (y:ys) i = abGo (listSet xs i (idx xs i + y)) ys (i + 1)

-- ================================================================
-- Ed25519 API
-- ================================================================

-- Clamp a 32-byte scalar per Ed25519 rules
clampScalar :: [Integer] -> [Integer]
clampScalar s =
    let s1 = listSet s 0 (bandB (idx s 0) 248)
        s2 = listSet s1 31 (borB (bandB (idx s1 31) 127) 64)
    in s2

-- Key pair generation from 32-byte seed
-- Returns (publicKey, secretKey) where secretKey = seed ++ publicKey (64 bytes)
ed25519Keypair :: ByteString -> (ByteString, ByteString)
ed25519Keypair seed =
    let h = bsUnpack (sha512 seed)
        a = clampScalar (take 32 h)
        pubPoint = scalarMult a basePoint
        pub = pointEncode pubPoint
    in (pub, bsConcat seed pub)

-- Sign a message
-- secret: 64-byte secret key (seed ++ public)
-- public: 32-byte public key
-- msg: message to sign
-- Returns 64-byte signature
ed25519Sign :: ByteString -> ByteString -> ByteString -> ByteString
ed25519Sign secret public msg =
    let h = bsUnpack (sha512 (bsSub secret 0 32))
        a = clampScalar (take 32 h)
        prefix = drop 32 h
        -- r = SHA-512(prefix || msg) mod L
        r = scReduce (bsUnpack (sha512 (bsConcat (bsPack prefix) msg)))
        -- R = r * B
        bigR = pointEncode (scalarMult r basePoint)
        -- k = SHA-512(R || public || msg) mod L
        k = scReduce (bsUnpack (sha512 (bsConcatList [bigR, public, msg])))
        -- S = (r + k * a) mod L
        s = scMulAdd k a r
    in bsConcat bigR (bsPack s)

-- Verify a signature
-- public: 32-byte public key
-- msg: message
-- sig: 64-byte signature
-- Returns True if valid
ed25519Verify :: ByteString -> ByteString -> ByteString -> Bool
ed25519Verify public msg sig =
    let encodedR = bsSub sig 0 32
        sBytes = bsUnpack (bsSub sig 32 32)
        bigA = pointDecode public
        -- k = SHA-512(R || public || msg) mod L
        k = scReduce (bsUnpack (sha512 (bsConcatList [encodedR, public, msg])))
        -- Check: [S]B == R + [k]A
        sB = scalarMult sBytes basePoint
        kA = scalarMult k bigA
        bigR = pointDecode encodedR
        rPlusKA = pointAdd bigR kA
    in pointEncode sB == pointEncode rPlusKA
