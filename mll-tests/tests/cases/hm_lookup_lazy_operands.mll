-- The fused `case hmLookup k m of …` (try_fused_hm_lookup_case) hands its
-- key and map to __mll_hm_slot, which forces both before the slot read,
-- so a lazy key and a lazy map handle (non-strict parameters, suspended
-- at the call) are read correctly. Pinned beside the ST inline read,
-- whose site reads its operands itself.
module Main where

m1 :: HashMap Int Int
m1 = hmInsert 1 11 (hmInsert 2 22 hmEmpty)

m2 :: HashMap Int Int
m2 = hmInsert 1 111 hmEmpty

lazyInt :: Int -> Int
lazyInt n = length (replicate n ())

pickMap :: Bool -> HashMap Int Int -> HashMap Int Int -> HashMap Int Int
pickMap b x y = if b then x else y

look :: Bool -> Int -> HashMap Int Int -> Int
look b k m = let r = case hmLookup k m of
                    Just v -> v
                    Nothing -> -1
             in if b then 0 else r

main :: IO ()
main = do
    print (look False (lazyInt 2) (pickMap (lazyInt 1 > 5) m1 m2))
    print (look False (lazyInt 1) (pickMap (lazyInt 1 < 5) m1 m2))
    print (look True (error "unused key") (error "unused map"))
    print (look False (lazyInt 2) (pickMap (lazyInt 1 < 5) m1 m2) * 2)
-- expect: -1
-- expect: 11
-- expect: 0
-- expect: 44
