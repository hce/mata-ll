-- Dijkstra's shortest paths with a leftist heap as the priority queue,
-- parent maps for path reconstruction, and heap sort as a by-product.
-- Leans on: a recursive heap type with rank bookkeeping, Ord-polymorphic
-- heap operations reused at two element types, Data.Map for distances,
-- parents and adjacency, Maybe-guarded relaxation, tuple ordering.
import Data.Map (Map)
import qualified Data.Map as M
import Data.Maybe (fromMaybe)

data Heap a = Empty | HNode Int a (Heap a) (Heap a)

rank :: Heap a -> Int
rank Empty = 0
rank (HNode r _ _ _) = r

merge :: Ord a => Heap a -> Heap a -> Heap a
merge Empty h = h
merge h Empty = h
merge h1@(HNode _ x l1 r1) h2@(HNode _ y l2 r2)
    | x <= y = make x l1 (merge r1 h2)
    | otherwise = make y l2 (merge h1 r2)
  where
    make v a b = if rank a >= rank b then HNode (rank b + 1) v a b else HNode (rank a + 1) v b a

push :: Ord a => a -> Heap a -> Heap a
push x = merge (HNode 1 x Empty Empty)

pop :: Ord a => Heap a -> Maybe (a, Heap a)
pop Empty = Nothing
pop (HNode _ x l r) = Just (x, merge l r)

fromList :: Ord a => [a] -> Heap a
fromList = foldr push Empty

heapSort :: Ord a => [a] -> [a]
heapSort xs = drain (fromList xs)
  where
    drain h = case pop h of
        Nothing -> []
        Just (x, rest) -> x : drain rest

size :: Heap a -> Int
size Empty = 0
size (HNode _ _ l r) = 1 + size l + size r

type Graph = Map Int [(Int, Int)]

mkGraph :: [(Int, Int, Int)] -> Graph
mkGraph edges = foldr add M.empty edges
  where add (a, b, w) g = M.insertWith (++) a [(b, w)] (M.insertWith (++) b [(a, w)] g)

dijkstra :: Graph -> Int -> (Map Int Int, Map Int Int)
dijkstra g source = go (push (0, source) Empty) (M.singleton source 0) M.empty
  where
    go heap dist parents = case pop heap of
        Nothing -> (dist, parents)
        Just ((d, v), rest)
            | d > M.findWithDefault d v dist -> go rest dist parents
            | otherwise ->
                let edges = fromMaybe [] (M.lookup v g)
                    (heap', dist', parents') = foldl (relax v d) (rest, dist, parents) edges
                in go heap' dist' parents'
    relax v d (heap, dist, parents) (w, weight) =
        let nd = d + weight
        in case M.lookup w dist of
            Just old | old <= nd -> (heap, dist, parents)
            _ -> (push (nd, w) heap, M.insert w nd dist, M.insert w v parents)

pathTo :: Map Int Int -> Int -> Int -> [Int]
pathTo parents source target = go target []
  where
    go v acc
        | v == source = source : acc
        | otherwise = case M.lookup v parents of
            Nothing -> []
            Just p -> go p (v : acc)

main :: IO ()
main = do
    let g = mkGraph [(1, 2, 7), (1, 3, 9), (1, 6, 14), (2, 3, 10), (2, 4, 15), (3, 4, 11), (3, 6, 2), (4, 5, 6), (5, 6, 9), (7, 8, 1)]
        (dist, parents) = dijkstra g 1
    print (M.toList dist)
    print (M.toList parents)
    mapM_ (\t -> putStrLn (show t <> ": " <> show (M.lookup t dist) <> " via " <> show (pathTo parents 1 t))) [1 .. 8]
    let (dist7, parents7) = dijkstra g 7
    print (M.toList dist7, pathTo parents7 7 8)
    print (heapSort [5, 3, 9, 1, 5, 7, 2, 8, 0, -4])
    print (heapSort ["pear", "apple", "fig", "banana"])
    print (heapSort ([] :: [Int]), heapSort [1])
    let h = fromList [(3, "c"), (1, "a"), (2, "b"), (1, "z")]
    print (size h, rank h, fmap fst (pop h))
    print (heapSort (concat (replicate 3 [4, 2, 6])))
    print (take 5 (heapSort [100, 99 .. 1]))
