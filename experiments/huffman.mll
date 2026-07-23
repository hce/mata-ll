-- Huffman coding: a self-checking compiler stress test.
--
-- Exercises: a recursive sum type (HTree) used at many positions, deeply
-- nested constructor patterns, manual Ordering comparators with sortBy,
-- association-list lookups, tree traversal with an accumulator, guards,
-- LBit bitwise ops, and ByteString pack/unpack.
--
-- The encode/decode roundtrip is its own oracle: decoded output must equal
-- the original input, so the program checks itself and fails (via assert ->
-- error) if anything is miscompiled.

import Data.List (sortBy, drop, replicate)

-- Bitwise FFI (Lua bit ops), declared locally as in examples/aestest.mll.
band   :: Int -> Int -> LuaPure "__mll_band" Int
bor    :: Int -> Int -> LuaPure "__mll_bor" Int
shiftL :: Int -> Int -> LuaPure "__mll_shl" Int
shiftR :: Int -> Int -> LuaPure "__mll_shr" Int

-- A Huffman tree: leaves carry a symbol and its frequency; internal nodes
-- carry the combined frequency of their subtrees.
data HTree = Leaf Int Int
           | Node Int HTree HTree

freq :: HTree -> Int
freq (Leaf _ f)   = f
freq (Node f _ _) = f

-- ── small list helpers (kept local to stress monomorphization) ──────────

takeN :: Int -> [a] -> [a]
takeN 0 _        = []
takeN _ []       = []
takeN n (x:xs)   = x : takeN (n - 1) xs

eqInts :: [Int] -> [Int] -> Bool
eqInts []     []     = True
eqInts (x:xs) (y:ys) = x == y && eqInts xs ys
eqInts _      _      = False

cmpFreq :: HTree -> HTree -> Ordering
cmpFreq a b = if freq a < freq b then LT else if freq a > freq b then GT else EQ

-- ── frequency table as an association list [(symbol, count)] ────────────

bump :: Int -> [(Int, Int)] -> [(Int, Int)]
bump s []            = [(s, 1)]
bump s ((k, c):rest)
  | k == s    = (k, c + 1) : rest
  | otherwise = (k, c) : bump s rest

freqTable :: [Int] -> [(Int, Int)]
freqTable = foldl (\acc s -> bump s acc) []

mkLeaf :: (Int, Int) -> HTree
mkLeaf (s, c) = Leaf s c

-- ── build the tree: repeatedly combine the two lowest-frequency trees ───

build :: [HTree] -> HTree
build ts = case sortBy cmpFreq ts of
  []         -> error "huffman: empty input"
  [t]        -> t
  (a:b:rest) -> build (Node (freq a + freq b) a b : rest)

-- ── code table: DFS accumulating the path (0 = left, 1 = right) ─────────

codeTable :: HTree -> [(Int, [Int])]
codeTable t = case t of
  Leaf s _ -> [(s, [0])]          -- single-symbol input: give it a 1-bit code
  _        -> go [] t
  where
    go prefix (Leaf s _)   = [(s, reverse prefix)]
    go prefix (Node _ l r) = go (0 : prefix) l ++ go (1 : prefix) r

lookupCode :: Int -> [(Int, [Int])] -> [Int]
lookupCode s []          = error "lookupCode: symbol not in table"
lookupCode s ((k, v):rest)
  | k == s    = v
  | otherwise = lookupCode s rest

encodeBits :: [(Int, [Int])] -> [Int] -> [Int]
encodeBits table = concatMap (\s -> lookupCode s table)

-- Decode exactly n symbols by walking from the root, restarting at each leaf.
decodeSyms :: HTree -> Int -> [Int] -> [Int]
decodeSyms tree n bits = go n tree bits
  where
    go 0 _            _      = []
    go k (Leaf s _)   rest   = s : go (k - 1) tree rest
    go k (Node _ l r) (b:bs) = if b == 0 then go k l bs else go k r bs
    go _ (Node _ _ _) []     = error "decode: ran out of bits"

-- ── bit <-> byte packing (exercises LBit + ByteString) ──────────────────

padRight :: Int -> [Int] -> [Int]
padRight n xs = xs ++ replicate (n - length xs) 0

packByte :: [Int] -> Int
packByte chunk = foldl (\acc b -> bor (shiftL acc 1) b) 0 (padRight 8 chunk)

packBits :: [Int] -> [Int]
packBits [] = []
packBits bs = packByte (takeN 8 bs) : packBits (drop 8 bs)

byteToBits :: Int -> [Int]
byteToBits b = map (\i -> band (shiftR b (7 - i)) 1) [0, 1, 2, 3, 4, 5, 6, 7]

unpackBits :: Int -> [Int] -> [Int]
unpackBits n bytes = takeN n (concatMap byteToBits bytes)

-- ── driver ──────────────────────────────────────────────────────────────

sample :: [Int]
sample = bsUnpack (bsFromString "she sells sea shells by the sea shore")

main :: IO ()
main = do
  let input   = sample
  let n       = length input
  let tree    = build (map mkLeaf (freqTable input))
  let table   = codeTable tree
  let bits    = encodeBits table input
  let decoded = decodeSyms tree n bits

  -- Oracle 1: the symbol-level roundtrip must be exact.
  assert (eqInts decoded input) "huffman: decode(encode(x)) == x"

  -- Oracle 2: bit-packing through bytes is lossless for the known bit count.
  let packed   = packBits bits
  let unpacked = unpackBits (length bits) packed
  assert (eqInts unpacked bits) "huffman: unpack(pack(bits)) == bits"

  -- Oracle 3: ByteString pack/unpack roundtrip on the packed bytes.
  let bytes = bsPack packed
  assert (eqInts (bsUnpack bytes) packed) "huffman: bsUnpack(bsPack(x)) == x"

  -- Oracle 4: and the bytes decode back to the original symbols.
  let bits2    = unpackBits (length bits) (bsUnpack bytes)
  let decoded2 = decodeSyms tree n bits2
  assert (eqInts decoded2 input) "huffman: decode through ByteString == x"

  putStrLn ("symbols:        " <> show n)
  putStrLn ("encoded bits:   " <> show (length bits))
  putStrLn ("packed bytes:   " <> show (bsLength bytes))
  putStrLn "all huffman roundtrip checks passed"
