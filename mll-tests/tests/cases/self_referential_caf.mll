-- A top-level value that refers to ITSELF must keep the self-reference, not
-- lose it to the Lua `local x = <reads x>` scoping gotcha. Cons-headed
-- self-refs (`0 : xs`) already worked; these are the general cases: a
-- self-reference passed by name through a function, nested under a call, or
-- as a lazy constructor field.

-- self-reference passed by name to a function whose body is a cons
myCons :: Int -> [Int] -> [Int]
myCons a b = a : b

ones :: [Int]
ones = myCons 1 ones

-- self-reference nested inside a call argument
incs :: [Int]
incs = map (+ 1) (0 : incs)

-- self-reference as a lazy constructor field
data Stream = S Int Stream

sHead :: Stream -> Int
sHead (S x _) = x

sTail :: Stream -> Stream
sTail (S _ r) = r

nats :: Stream
nats = go 0
  where go n = S n (go (n + 1))

-- a genuinely self-referential Stream value
repeatS :: Int -> Stream
repeatS x = let s = S x s in s

main :: IO ()
main = do
    assert (take 3 ones == [1, 1, 1]) "ones = myCons 1 ones"
    assert (take 4 incs == [1, 2, 3, 4]) "incs = map (+1) (0 : incs)"
    assert (sHead (sTail (repeatS 7)) == 7) "self-referential Stream field"
    assert (sHead (sTail (sTail nats)) == 2) "mutually-recursive where Stream"
    putStrLn "self-referential CAFs ok"
-- expect: self-referential CAFs ok
