-- Graph algorithms over an adjacency map: breadth-first levels, depth-first
-- order, connected components, Kahn's topological sort, cycle detection,
-- and shortest unweighted paths with parent tracking.
-- Leans on: Data.Map with Int keys and list values, Data.Set for visited
-- sets, explicit worklist recursion, Maybe-returning searches, foldr/foldl
-- accumulation over map contents.
import Data.Map (Map)
import qualified Data.Map as M
import qualified Data.Set as S
import Data.Maybe (fromMaybe)

type Graph = Map Int [Int]

mkGraph :: [(Int, Int)] -> Graph
mkGraph edges = foldl addEdge M.empty edges
  where addEdge g (a, b) = M.insertWith (\new old -> old ++ new) a [b] (M.insertWith (\new old -> old ++ new) b [] g)

undirected :: [(Int, Int)] -> Graph
undirected edges = mkGraph (edges ++ [(b, a) | (a, b) <- edges])

neighbours :: Graph -> Int -> [Int]
neighbours g v = fromMaybe [] (M.lookup v g)

-- Breadth-first search: the list of levels from the start.
bfsLevels :: Graph -> Int -> [[Int]]
bfsLevels g start = go [start] (S.singleton start)
  where
    go [] _ = []
    go frontier seen =
        let next = expand frontier seen []
            seen' = foldr S.insert seen next
        in frontier : go next seen'
    expand [] _ acc = reverse acc
    expand (v : vs) seen acc =
        let fresh = [w | w <- neighbours g v, not (S.member w seen), not (elem w acc)]
        in expand vs (foldr S.insert seen fresh) (reverse fresh ++ acc)

-- Depth-first preorder with an explicit stack.
dfsOrder :: Graph -> Int -> [Int]
dfsOrder g start = go [start] S.empty
  where
    go [] _ = []
    go (v : stack) seen
        | S.member v seen = go stack seen
        | otherwise = v : go (neighbours g v ++ stack) (S.insert v seen)

components :: Graph -> [[Int]]
components g = go (M.keys g) S.empty
  where
    go [] _ = []
    go (v : vs) seen
        | S.member v seen = go vs seen
        | otherwise =
            let comp = dfsOrder g v
            in comp : go vs (foldr S.insert seen comp)

-- Shortest path by BFS with a parent map; Nothing when unreachable.
shortestPath :: Graph -> Int -> Int -> Maybe [Int]
shortestPath g from to = go [from] (M.singleton from from)
  where
    go [] _ = Nothing
    go (v : queue) parents
        | v == to = Just (walk to parents)
        | otherwise =
            let fresh = [w | w <- neighbours g v, not (M.member w parents)]
                parents' = foldl (\m w -> M.insert w v m) parents fresh
            in go (queue ++ fresh) parents'
    walk v parents
        | v == from = [from]
        | otherwise = walk (M.findWithDefault from v parents) parents ++ [v]

inDegrees :: Graph -> Map Int Int
inDegrees g = foldr (\w m -> M.insertWith (+) w 1 m) zeros (concat (M.elems g))
  where zeros = M.map (\_ -> 0) g

-- Kahn's algorithm; Left the leftover vertices when a cycle blocks it.
topoSort :: Graph -> Either [Int] [Int]
topoSort g = go (M.keys (M.filter (== 0) degrees)) degrees []
  where
    degrees = inDegrees g
    go [] remaining acc
        | M.size (M.filter (> 0) remaining) == 0 = Right (reverse acc)
        | otherwise = Left (M.keys (M.filter (> 0) remaining))
    go (v : ready) remaining acc =
        let (remaining', newlyReady) = foldl relax (M.insert v (-1) remaining, []) (neighbours g v)
        in go (ready ++ reverse newlyReady) remaining' (v : acc)
    relax (m, ready) w =
        let d = M.findWithDefault 0 w m - 1
        in (M.insert w d m, if d == 0 then w : ready else ready)

hasCycle :: Graph -> Bool
hasCycle g = case topoSort g of
    Left _ -> True
    Right _ -> False

showGraph :: Graph -> String
showGraph g = mconcat [show v <> " -> " <> show ws <> "\n" | (v, ws) <- M.toList g]

main :: IO ()
main = do
    let social = undirected [(1, 2), (1, 3), (2, 4), (3, 4), (4, 5), (6, 7), (8, 8)]
    putStr (showGraph social)
    print (bfsLevels social 1)
    print (dfsOrder social 1)
    print (components social)
    print (shortestPath social 1 5)
    print (shortestPath social 5 1)
    print (shortestPath social 1 7)
    print (shortestPath social 6 6)
    let tasks = mkGraph [(1, 2), (1, 3), (2, 4), (3, 4), (4, 5), (3, 5), (6, 5)]
    putStr (showGraph tasks)
    print (M.toList (inDegrees tasks))
    print (topoSort tasks)
    print (hasCycle tasks)
    let cyclic = mkGraph [(1, 2), (2, 3), (3, 1), (3, 4)]
    print (topoSort cyclic)
    print (hasCycle cyclic)
    print (bfsLevels cyclic 4)
    print (map (\c -> (head c, length c)) (components (undirected [(i, i + 1) | i <- [1 .. 9], i /= 5])))
