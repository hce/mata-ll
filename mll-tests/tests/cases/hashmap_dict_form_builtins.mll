-- W24 (H2, H3): the hm* builtins in a dictionary-form body and at an
-- existential key. `keyed` is used at 20 key types, past the
-- specialization limit, so every keyed operation in it runs with the key
-- type's encoder and order read off the Hashable dictionary parameter,
-- and hmKeys/hmValues/hmToList read the order off the map itself; `Box`
-- captures the dictionary of its hidden key type at construction. The
-- builtins have no GHC twin: the expectations are Data.Map's semantics,
-- pinned from a hand-written containers twin.
module Main where

data Box = forall k. Hashable k => Box k

numbered :: [k] -> Int -> [(k, Int)]
numbered [] _ = []
numbered (k : ks) i = (k, i) : numbered ks (i + 1)

keyed :: Hashable k => [k] -> k -> (Maybe Int, Bool, Int, [k], [Int], [(k, Int)], Int)
keyed ks k =
    let m = hmFromList (numbered ks 1)
        m2 = hmInsert k 0 m
    in (hmLookup k m, hmMember k m2, hmSize (hmDelete k m2), hmKeys m2, hmValues m2, hmToList (hmDelete (head ks) m2), hmSize m)

boxed :: Box -> (Int, Bool, Int)
boxed (Box k) =
    let m = hmInsert k 1 (hmInsert k 2 hmEmpty)
    in (hmSize m, hmMember k m, hmSize (hmDelete k m))

main :: IO ()
main = do
    print (keyed [3, 1, 2 :: Int] 2)
    print (keyed [2.5, 1.5 :: Number] 1.5)
    print (keyed ["b", "a"] "c")
    print (keyed [True, False] False)
    print (keyed [[2], [1, 2], [1 :: Int]] [1, 2])
    print (keyed [Just (2 :: Int), Nothing, Just 1] Nothing)
    print (keyed [(1 :: Int, 2 :: Int), (0, 5)] (0, 5))
    print (keyed [["x", "y"], ["x"]] ["x"])
    print (keyed [Just "b", Nothing] (Just "b"))
    print (keyed [(2 :: Int, "b"), (1, "z")] (1, "z"))
    print (keyed [[True], [False, True]] [True])
    print (keyed [Just False, Just True] Nothing)
    print (keyed [(True, 1 :: Int), (False, 2)] (True, 1))
    print (keyed [[[1 :: Int]], [[]], []] [[]])
    print (keyed [Just (Just (1 :: Int)), Just Nothing] (Just Nothing))
    print (keyed [(1 :: Int, 2 :: Int, 3 :: Int), (1, 2, 0)] (1, 2, 3))
    print (keyed [[1.5 :: Number], [0.5]] [0.5])
    print (keyed [Just [1 :: Int], Nothing] (Just [1]))
    print (keyed [(1 :: Int, [2 :: Int]), (1, [])] (1, []))
    print (keyed [map (+ 1) [1, 2 :: Int], [1 .. 3], [4] ++ [5]] (take 2 [2 ..]))
    print (boxed (Box (1 :: Int)), boxed (Box [1, 2 :: Int]), boxed (Box (Just "s")), boxed (Box (1 :: Int, Just (2 :: Int))))
    print (boxed (Box [[1 :: Int]]), boxed (Box (map (* 2) [1, 2 :: Int])), boxed (Box (Just (Just (3 :: Int)))), boxed (Box (True, "t", 2.5 :: Number)))

-- expect: (Just 3,True,2,[1,2,3],[2,0,1],[(1,2),(2,0)],3)
-- expect: (Just 2,True,1,[1.5,2.5],[0,1],[(1.5,0)],2)
-- expect: (Nothing,True,2,["a","b","c"],[2,1,0],[("a",2),("c",0)],2)
-- expect: (Just 2,True,1,[False,True],[0,1],[(False,0)],2)
-- expect: (Just 2,True,2,[[1],[1,2],[2]],[3,0,1],[([1],3),([1,2],0)],3)
-- expect: (Just 2,True,2,[Nothing,Just 1,Just 2],[0,3,1],[(Nothing,0),(Just 1,3)],3)
-- expect: (Just 2,True,1,[(0,5),(1,2)],[0,1],[((0,5),0)],2)
-- expect: (Just 2,True,1,[["x"],["x","y"]],[0,1],[(["x"],0)],2)
-- expect: (Just 1,True,1,[Nothing,Just "b"],[2,0],[(Nothing,2)],2)
-- expect: (Just 2,True,1,[(1,"z"),(2,"b")],[0,1],[((1,"z"),0)],2)
-- expect: (Just 1,True,1,[[False,True],[True]],[2,0],[([False,True],2)],2)
-- expect: (Nothing,True,2,[Nothing,Just False,Just True],[0,1,2],[(Nothing,0),(Just True,2)],2)
-- expect: (Just 1,True,1,[(False,2),(True,1)],[2,0],[((False,2),2)],2)
-- expect: (Just 2,True,2,[[],[[]],[[1]]],[3,0,1],[([],3),([[]],0)],3)
-- expect: (Just 2,True,1,[Just Nothing,Just (Just 1)],[0,1],[(Just Nothing,0)],2)
-- expect: (Just 1,True,1,[(1,2,0),(1,2,3)],[2,0],[((1,2,0),2)],2)
-- expect: (Just 2,True,1,[[0.5],[1.5]],[0,1],[([0.5],0)],2)
-- expect: (Just 1,True,1,[Nothing,Just [1]],[2,0],[(Nothing,2)],2)
-- expect: (Just 2,True,1,[(1,[]),(1,[2])],[0,1],[((1,[]),0)],2)
-- expect: (Just 1,True,2,[[1,2,3],[2,3],[4,5]],[2,0,3],[([1,2,3],2),([4,5],3)],3)
-- expect: ((1,True,0),(1,True,0),(1,True,0),(1,True,0))
-- expect: ((1,True,0),(1,True,0),(1,True,0),(1,True,0))
