-- Huffman coding: a frequency table over byte codes, a tree built by
-- repeatedly merging the two lightest subtrees, a code table, and an
-- encoder/decoder round trip on bit lists.
-- Leans on: Data.Map with Int keys and list-of-Bool values, insertion into
-- a sorted list as a priority queue, tree recursion, Bool lists rendered
-- as strings, LString, show of nested tuples.
import LString
import Data.Map (Map)
import qualified Data.Map as M
import Data.List (sortBy)

data HTree = HLeaf Int Int | HNode Int HTree HTree
    deriving Show

weight :: HTree -> Int
weight (HLeaf w _) = w
weight (HNode w _ _) = w

frequencies :: String -> Map Int Int
frequencies s = foldl (\m c -> M.insertWith (+) c 1 m) M.empty (strToInts s)

-- Deterministic tie-breaking: lighter first, then the smaller code.
byWeight :: HTree -> HTree -> Ordering
byWeight a b = case compare (weight a) (weight b) of
    EQ -> compare (minCode a) (minCode b)
    other -> other

minCode :: HTree -> Int
minCode (HLeaf _ c) = c
minCode (HNode _ l r) = min (minCode l) (minCode r)

buildTree :: Map Int Int -> Maybe HTree
buildTree freqs = go (sortBy byWeight [HLeaf w c | (c, w) <- M.toList freqs])
  where
    go [] = Nothing
    go [t] = Just t
    go (a : b : rest) = go (insertSorted (HNode (weight a + weight b) a b) rest)

insertSorted :: HTree -> [HTree] -> [HTree]
insertSorted t [] = [t]
insertSorted t (x : xs) = case byWeight t x of
    GT -> x : insertSorted t xs
    _ -> t : x : xs

codeTable :: HTree -> Map Int [Bool]
codeTable (HLeaf _ c) = M.singleton c [False]
codeTable t = M.fromList (walk [] t)
  where
    walk path (HLeaf _ c) = [(c, reverse path)]
    walk path (HNode _ l r) = walk (False : path) l ++ walk (True : path) r

encode :: Map Int [Bool] -> String -> Maybe [Bool]
encode table s = fmap concat (mapM (\c -> M.lookup c table) (strToInts s))

decode :: HTree -> [Bool] -> String
decode (HLeaf _ c) bits = mconcat (replicate (length bits) (strChar c))
decode tree bits = go tree bits
  where
    go (HLeaf _ c) [] = strChar c
    go (HLeaf _ c) rest = strChar c <> go tree rest
    go (HNode _ _ _) [] = ""
    go (HNode _ l r) (b : rest) = go (if b then r else l) rest

bitsToString :: [Bool] -> String
bitsToString = mconcat . map (\b -> if b then "1" else "0")

showCode :: (Int, [Bool]) -> String
showCode (c, bits) = strChar c <> "=" <> bitsToString bits

report :: String -> IO ()
report s = do
    putStrLn ("text: " <> show s)
    let freqs = frequencies s
    print (M.toList freqs)
    case buildTree freqs of
        Nothing -> putStrLn "empty input"
        Just tree -> do
            let table = codeTable tree
            putStrLn ("codes: " <> mconcat (map (\e -> showCode e <> " ") (M.toList table)))
            case encode table s of
                Nothing -> putStrLn "encoding failed"
                Just bits -> do
                    putStrLn ("bits: " <> bitsToString bits)
                    putStrLn ("length: " <> show (length bits) <> " vs " <> show (8 * strLen s))
                    let back = decode tree bits
                    putStrLn ("decoded: " <> show back <> " ok=" <> show (back == s))
            print (encode table "zzz")

main :: IO ()
main = do
    report "abracadabra"
    report "mississippi river"
    report "aaaa"
    report ""
    report "the quick brown fox jumps over the lazy dog"
    case buildTree (frequencies "abbccc") of
        Nothing -> putStrLn "none"
        Just t -> print t
