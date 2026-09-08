-- Word frequencies: tokenizes a text into lower-case words and builds
-- several maps over them (word counts, bigrams keyed by tuples, a letter
-- histogram), then reports rankings.
-- Leans on: Data.Map with String, Int and tuple keys (insertWith, adjust,
-- findWithDefault, foldrWithKey, filter, union, delete), sortBy with a
-- composite order, LString, string building.
import LString
import Data.Map (Map)
import qualified Data.Map as M
import Data.List (sortBy)

text :: String
text =
    "The quick brown fox jumps over the lazy dog. The dog sleeps; the fox runs.\n"
    <> "A fox is quick, a dog is lazy, and the brown fox is the quickest fox.\n"
    <> "Over and over the fox jumps: the lazy dog never moves."

isLetter :: Int -> Bool
isLetter c = (c >= 65 && c <= 90) || (c >= 97 && c <= 122)

toLower :: Int -> Int
toLower c = if c >= 65 && c <= 90 then c + 32 else c

fromCodes :: [Int] -> String
fromCodes cs = mconcat (map strChar cs)

-- Split on anything that is not a letter.
wordsOf :: String -> [String]
wordsOf s = go (strToInts s)
  where
    go [] = []
    go cs = case span isLetter (dropWhile (not . isLetter) cs) of
        ([], _) -> []
        (w, rest) -> fromCodes (map toLower w) : go rest

countWords :: [String] -> Map String Int
countWords = foldl (\m w -> M.insertWith (+) w 1 m) M.empty

bigrams :: [String] -> Map (String, String) Int
bigrams ws = foldl (\m p -> M.insertWith (+) p 1 m) M.empty (zip ws (drop 1 ws))

letterHistogram :: [String] -> Map Int Int
letterHistogram ws = foldl (\m c -> M.insertWith (+) c 1 m) M.empty (concatMap strToInts ws)

-- Highest count first, ties alphabetically.
byCount :: (String, Int) -> (String, Int) -> Ordering
byCount (wa, ca) (wb, cb) = case compare cb ca of
    EQ -> compare wa wb
    other -> other

bar :: Int -> String
bar n = mconcat (replicate n "*")

main :: IO ()
main = do
    let ws = wordsOf text
        counts = countWords ws
    putStrLn ("words: " <> show (length ws) <> ", distinct: " <> show (M.size counts))
    putStrLn "top 8:"
    mapM_ (\(w, c) -> putStrLn ("  " <> w <> " " <> show c)) (take 8 (sortBy byCount (M.toList counts)))
    let singles = M.filter (== 1) counts
    putStrLn ("hapax legomena: " <> show (M.size singles) <> " " <> show (M.keys singles))
    putStrLn ("fox: " <> show (M.findWithDefault 0 "fox" counts) <> ", cat: " <> show (M.findWithDefault 0 "cat" counts))
    putStrLn ("member dog: " <> show (M.member "dog" counts) <> ", member cat: " <> show (M.member "cat" counts))
    let longest = foldr (\w best -> if strLen w > strLen best then w else best) "" (M.keys counts)
    putStrLn ("longest word: " <> longest)
    putStrLn ("total via foldrWithKey: " <> show (M.foldrWithKey (\_ c acc -> c + acc) 0 counts))
    -- Adjust, delete and union.
    let adjusted = M.adjust (* 100) "the" (M.delete "a" counts)
    putStrLn ("adjusted the: " <> show (M.lookup "the" adjusted) <> ", a: " <> show (M.lookup "a" adjusted))
    let merged = M.union (M.fromList [("the", -1), ("zebra", 9)]) adjusted
    putStrLn ("union keeps left: " <> show (M.lookup "the" merged) <> ", adds: " <> show (M.lookup "zebra" merged))
    putStrLn "bigrams seen more than once:"
    let bg = M.filter (> 1) (bigrams ws)
    mapM_ (\((a, b), c) -> putStrLn ("  " <> a <> " " <> b <> ": " <> show c)) (M.toList bg)
    putStrLn "letters:"
    let hist = letterHistogram ws
    mapM_ (\(c, n) -> putStrLn ("  " <> strChar c <> " " <> bar n)) (M.toList hist)
    putStrLn ("letters total: " <> show (sum (M.elems hist)))
    putStrLn (mconcat (map (<> ",") (take 5 (M.keys counts))))
