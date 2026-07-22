-- Where-bound function groups hold Lua function values from their group
-- assignment on, never thunks: calls to them — including the group's own
-- mutual recursion — skip the __force, and a `_warg` entry rebind is not
-- re-forced by the clause conditions. The laziness-contract cases
-- (non_strict, list_element_laziness, tuple_field_laziness) guard the
-- other direction: nothing here may become MORE eager.

interleave :: [Integer] -> [Integer] -> [Integer]
interleave xs ys = go xs ys
  where
    go [] bs = bs
    go (a:as) bs = a : swap bs as
    swap [] as = as
    swap (b:bs) as = b : go as bs

-- A value binding named like a function stays a thunk and stays forced.
lazyVal :: Integer -> Integer
lazyVal n = v + 1
  where v = n * 2

main :: IO ()
main = do
    assert (interleave [1,3,5] [2,4,6] == [1,2,3,4,5,6]) "mutual where-group recursion"
    assert (lazyVal 20 == 41) "where value binding intact"
