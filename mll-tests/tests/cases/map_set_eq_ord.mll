-- W64 (H8): Eq and Ord of Data.Map's Map and Data.Set's Set are
-- containers' instances — `(Eq k, Eq v) => Eq (Map k v)` is "same size
-- and same ascending entry list", `(Ord k, Ord v) => Ord (Map k v)` is
-- `compare` of the ascending entry lists with the other six methods the
-- class defaults over it. `compare` on two maps used to be rejected with
-- "No instance for 'Ord (HashMap Int String)'" (and `==` likewise).
-- Pinned against the containers twin: Int, String, tuple, list, Maybe and
-- Number keys; empty maps, prefix maps, equal maps built in different
-- insertion orders and maps that are versions of one persistent store;
-- maps inside lists, tuples, Maybe, Either, other maps and a type that
-- derives Eq and Ord; sort/maximum/minimum/elem/lookup over maps; and the
-- size test that answers before any entry is compared. `rel` is used at
-- few types and stays specialized; `relD` is used at more types than the
-- specialization limit (`widen`) and runs every comparison through an Ord
-- dictionary; `relPair` hands it the dictionary an existential captured,
-- and `relEntries` and `relMembers` compare maps and sets whose key and
-- value types are hidden, where the map instance is composed from the
-- captured key and value dictionaries. NaN-bearing maps are compared
-- with the methods whose class default reads `compare x y` (`<` and `>=`
-- at a NaN are the class defaults' own difference from base, not the map
-- instance's).
module Main where

import qualified Data.Map as M
import qualified Data.Set as S

data Shelf = Shelf String (M.Map Int String) (S.Set String)
    deriving (Eq, Ord)

data Pair = forall a. Ord a => Pair a a

data Entries = forall k v. (Ord k, Ord v) => Entries (M.Map k v) (M.Map k v)

data Members = forall a. Ord a => Members (S.Set a) (S.Set a)

rel :: Ord a => a -> a -> String
rel x y = show (compare x y) <> " " <> show [x == y, x /= y, x < y, x <= y, x > y, x >= y, max x y == y, min x y == x]

relD :: Ord a => a -> a -> String
relD x y = show (compare x y) <> " " <> show [x == y, x /= y, x < y, x <= y, x > y, x >= y, max x y == y, min x y == x]

relPair :: Pair -> String
relPair (Pair x y) = relD x y

relEntries :: Entries -> String
relEntries (Entries x y) = show (compare x y) <> " " <> show [x == y, x /= y, x < y, x <= y, x > y, x >= y, max x y == y, min x y == x]

relMembers :: Members -> String
relMembers (Members x y) = show (compare x y) <> " " <> show [x == y, x /= y, x < y, x <= y, x > y, x >= y, max x y == y, min x y == x]

agree :: (Ord k, Ord v) => M.Map k v -> M.Map k v -> (Bool, Ordering)
agree x y = (x == y, compare x y)

widen :: [String]
widen =
    [ relD (1 :: Int) 2, relD (2.5 :: Number) 1.5, relD "a" "b", relD True False, relD () ()
    , relD LT GT, relD [1 :: Int] [], relD (Just (1 :: Int)) Nothing, relD (1 :: Int, "a") (1, "b")
    , relD ["a"] ["a"], relD (Just "x") (Just "y"), relD (1 :: Int, 2 :: Int, 3 :: Int) (1, 2, 4)
    , relD [[1 :: Int]] [[2]], relD (Just (Just True)) (Just Nothing), relD (True, "t") (False, "t")
    , relD [2.5 :: Number] [1.5], relD (Just (1 :: Int, 2 :: Int)) Nothing, relD [True] [False]
    , relD (Left 1 :: Either Int String) (Right "r")
    ]

boom :: Int
boom = error "an entry was compared"

main :: IO ()
main = do
    let a = M.fromList [(1 :: Int, "x"), (2, "y"), (3, "z")]
        b = M.insert 2 "y" (M.insert 3 "z" (M.insert 1 "x" M.empty))
        c = M.fromList [(1 :: Int, "x"), (2, "y")]
        d = M.fromList [(1 :: Int, "x"), (2, "yy"), (3, "a")]
        e = M.fromList [(0 :: Int, "zzz")]
        z = M.empty :: M.Map Int String
    print (compare a b, a == b, a /= b, a < b, a <= b, a > b, a >= b)
    print (compare c a, c == a, c /= a, c < a, c <= a, c > a, c >= a)
    print (M.toList (max a d), M.toList (min a d), M.toList (max c e), M.toList (min z a), M.toList (max z a))
    mapM_ putStrLn [rel a b, rel c a, rel a c, rel a d, rel e a, rel z z, rel z a, rel a z]
    print (agree a b, agree c a, agree d a)

    let sa = M.fromList [("pear", 3 :: Int), ("apple", 1), ("fig", 2)]
        sb = M.fromList [("apple", 1 :: Int), ("fig", 2), ("pear", 3)]
        sc = M.fromList [("apple", 1 :: Int), ("fig", 9)]
        sd = M.fromList [("apple", 1 :: Int), ("banana", 0), ("pear", 3)]
        sz = M.empty :: M.Map String Int
    mapM_ putStrLn [rel sa sb, rel sc sa, rel sd sa, rel sz sa, rel sa sz]
    print (agree sa sb, agree sc sa, M.toList (max sc sa), M.toList (min sd sa))

    let ta = M.fromList [((1 :: Int, "b"), True), ((1, "a"), False), ((0, "z"), True)]
        tb = M.insert (0, "z") True (M.insert (1, "a") False (M.insert (1, "b") True M.empty))
        tc = M.fromList [((0 :: Int, "z"), True), ((1, "a"), True)]
        td = M.fromList [((0 :: Int, "z"), True)]
        tz = M.empty :: M.Map (Int, String) Bool
    mapM_ putStrLn [rel ta tb, rel tc ta, rel td ta, rel tz td, rel tz tz]
    print (ta == tb, compare tc ta, td < ta, tz >= td, M.toList (max tc ta), M.toList (min td tz))

    let ia = S.fromList [3, 1, 2 :: Int]
        ib = S.insert 2 (S.insert 1 (S.insert 3 S.empty))
        ic = S.fromList [1, 2 :: Int]
        ie = S.fromList [1, 4 :: Int]
        iz = S.empty :: S.Set Int
    mapM_ putStrLn [rel ia ib, rel ic ia, rel ie ia, rel iz ia, rel iz iz]
    print (ia == ib, compare ic ia, ie > ia, iz < ia, S.toList (max ia ie), S.toList (min ia ie))
    let wa = S.fromList ["b", "a"]
        wb = S.fromList ["a", "b", "c"]
        wc = S.fromList ["a", "c"]
    mapM_ putStrLn [rel wa wb, rel wc wb, rel wb wb, rel (S.insert "c" wa) wb]
    let pa = S.fromList [(1 :: Int, 2 :: Int), (0, 9)]
        pb = S.fromList [(0 :: Int, 9 :: Int), (1, 2)]
        pc = S.fromList [(0 :: Int, 9 :: Int), (1, 3)]
    mapM_ putStrLn [rel pa pb, rel pa pc, rel (S.delete (1, 2) pa) pb]
    print (S.toList (max pa pc), S.toList (min pa pc), S.toList (max wa wc))

    let na = M.fromList [(1.5 :: Number, 2.5 :: Number), (-1.0, 0.5)]
        nb = M.fromList [(-1.0 :: Number, 0.5 :: Number), (1.5, 2.5)]
        nc = M.fromList [(-1.0 :: Number, 0.5 :: Number), (1.5, 3.5)]
    mapM_ putStrLn [rel na nb, rel na nc, rel nc na]
    let zp = M.fromList [(1 :: Int, 0.0 :: Number)]
        zn = M.fromList [(1 :: Int, -0.0 :: Number)]
        kp = M.fromList [(0.0 :: Number, "z")]
        kn = M.fromList [(-0.0 :: Number, "z")]
    print (zp == zn, compare zp zn, M.toList (max zp zn), M.toList (min zp zn))
    print (kp == kn, compare kp kn, kp < kn, kp >= kn)
    let nan = 0 / 0 :: Number
        vn = M.fromList [(1 :: Int, nan)]
        vo = M.fromList [(1 :: Int, 1.0 :: Number)]
        kq = M.insert nan "n" (M.fromList [(1.0 :: Number, "a")])
        sq = S.fromList [nan]
    print (vn == vn, vn /= vn, compare vn vn, vn <= vn, vn > vn)
    print (vn == vo, compare vn vo, compare vo vn, vo <= vn, vo > vn)
    print (M.toList (max vo vn), M.toList (min vo vn), M.toList (max vn vo), M.toList (min vn vo))
    print (kq == kq, compare kq kq, M.size kq, sq == sq, compare sq sq)

    let v0 = M.fromList [(1 :: Int, "a"), (2, "b")]
        v1 = M.insert 3 "c" v0
        v2 = M.delete 1 v1
        v3 = M.insert 1 "a" v2
        v4 = M.insert 2 "B" v1
    mapM_ putStrLn [rel v1 v3, rel v0 v1, rel v2 v3, rel v4 v1, rel v0 (M.delete 3 v3)]
    print (M.toList v0, M.toList v1, M.toList v2, M.toList v3, M.toList v4)
    let w0 = M.fromList [(1 :: Int, [0 :: Int])]
        w1 = M.insert 2 [M.size w0] w0
        w2 = M.insert 2 [1] w0
        w3 = M.insert 2 [M.size w1] w0
    print (w1 == w2, compare w1 w2, w3 == w2, compare w3 w2, compare w2 w3)
    print (M.toList w0, M.toList w1, M.toList w2, M.toList w3)
    let g0 = M.fromList [((0 :: Int, 0 :: Int), "o")]
        g1 = M.insert (0, 1) "n" g0
        g2 = M.insert (1, 0) "e" g0
        g3 = M.delete (1, 0) (M.insert (0, 1) "n" g2)
    print (g1 == g3, compare g1 g2, compare g2 g1, g0 < g1, M.toList g3, M.toList g2)

    let maps = [a, c, e, z, d, b]
    print (map M.toList (sort maps))
    print (map M.toList (sortBy (\x y -> compare y x) maps))
    print (M.toList (maximum maps), M.toList (minimum maps))
    print (elem c maps, elem (M.fromList [(9 :: Int, "q")]) maps, lookup c [(a, "a"), (c, "c")], lookup z [(a, "a")])
    print (map S.toList (sort [ie, ia, iz, ic, ib]))
    mapM_ putStrLn [rel [a, c] [b, c], rel [a] [a, z], rel [c, a] [a]]
    mapM_ putStrLn [rel (Just a) (Just b), rel Nothing (Just z), rel (Just c) (Just a)]
    mapM_ putStrLn [rel (a, ia) (b, ib), rel (a, ic) (b, ia), rel (c, ia) (a, iz)]
    let oa = M.fromList [("k", a), ("j", z)]
        ob = M.fromList [("j", z), ("k", b)]
        oc = M.fromList [("j", z), ("k", c)]
    mapM_ putStrLn [rel oa ob, rel oc oa, rel oa (M.delete "j" oa)]
    mapM_ putStrLn
        [ rel (Shelf "s" a wa) (Shelf "s" b (S.fromList ["a", "b"]))
        , rel (Shelf "s" c wa) (Shelf "s" a S.empty)
        , rel (Shelf "s" a wa) (Shelf "s" a wb)
        , rel (Shelf "r" a wb) (Shelf "s" z S.empty)
        ]

    mapM_ putStrLn widen
    mapM_ putStrLn [relD a b, relD c a, relD a c, relD a d, relD e a, relD z z, relD z a, relD a z]
    mapM_ putStrLn [relD sa sb, relD sc sa, relD sd sa, relD sz sa, relD sa sz]
    mapM_ putStrLn [relD ta tb, relD tc ta, relD td ta, relD tz td, relD tz tz]
    mapM_ putStrLn [relD ia ib, relD ic ia, relD ie ia, relD iz ia, relD iz iz]
    mapM_ putStrLn [relD wa wb, relD wc wb, relD pa pb, relD pa pc]
    mapM_ putStrLn [relD na nb, relD na nc, relD v1 v3, relD v2 v3, relD g1 g3, relD g1 g2]
    let la = M.fromList [([1, 2 :: Int], "x"), ([], "e")]
        lb = M.fromList [([] :: [Int], "e"), ([1, 2], "x"), ([1], "y")]
        ma = M.fromList [(Just (2 :: Int), 1 :: Int), (Nothing, 0)]
        mb = M.insert Nothing 0 (M.insert (Just 2) 1 M.empty)
    mapM_ putStrLn [relD la lb, relD lb la, relD la la, relD ma mb, relD ma (M.delete Nothing mb)]
    mapM_ putStrLn [relD [a, c] [b, c], relD [a] [a, z], relD (Just a) (Just b), relD Nothing (Just z)]
    mapM_ putStrLn [relD (a, ia) (b, ib), relD (c, ia) (a, iz), relD oa ob, relD oc oa]
    mapM_ putStrLn
        [ relD (Shelf "s" a wa) (Shelf "s" b (S.fromList ["a", "b"]))
        , relD (Shelf "s" a wa) (Shelf "s" a wb)
        , relD (Left a :: Either (M.Map Int String) (S.Set Int)) (Right ia)
        , relD (Right ic :: Either (M.Map Int String) (S.Set Int)) (Right ia)
        , relD (Left c :: Either (M.Map Int String) (S.Set Int)) (Left a)
        ]

    mapM_ (putStrLn . relPair)
        [Pair a b, Pair c a, Pair a d, Pair z a, Pair ia ic, Pair ta tc, Pair oa oc, Pair [a] [c], Pair (Just z) Nothing, Pair na nc]
    mapM_ (putStrLn . relEntries)
        [Entries a b, Entries c a, Entries a d, Entries sa sc, Entries ta tc, Entries na nc, Entries oa oc, Entries z z, Entries tz td, Entries la lb, Entries v1 v3]
    mapM_ (putStrLn . relMembers)
        [Members ia ib, Members ic ia, Members ie ia, Members wa wb, Members pa pc, Members iz ia, Members iz iz]

    let lazyA = M.fromList [(1 :: Int, [boom])]
        lazyB = M.fromList [(1 :: Int, [boom]), (2, [])]
    print (lazyA == lazyB, lazyA /= lazyB, compare lazyA (M.fromList [(0 :: Int, [boom])]), lazyB < M.fromList [(2 :: Int, [boom])])
