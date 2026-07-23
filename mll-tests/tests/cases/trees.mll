-- Recursive data types and tree operations

data Tree = Branch Tree Tree | Leaf Int

depth :: Tree -> Int
depth (Leaf _) = 0
depth (Branch a b) = 1 + max (depth a) (depth b)

main :: IO ()
main = do
    assert (depth (Leaf 1) == 0) "leaf depth"
    assert (depth (Branch (Leaf 1) (Leaf 2)) == 1) "branch depth 1"
    assert (depth (Branch (Branch (Leaf 1) (Leaf 2)) (Leaf 3)) == 2) "nested depth 2"
    let deep = Branch (Branch (Branch (Leaf 1) (Leaf 2)) (Leaf 3)) (Leaf 4)
    assert (depth deep == 3) "nested depth 3"
