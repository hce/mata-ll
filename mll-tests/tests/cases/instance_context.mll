-- Regression (SPEC "Typeclass instances"): a user instance with a context,
-- `instance Show a => Show (Tree a)`, must bring `Show a` into scope for the
-- method bodies (the recursive/element `show` calls) and demand `Show Int`
-- when the instance is used at `Tree Int`. The instance context used to be
-- parsed and silently discarded, so this failed with a spurious
-- missing-context error inside the instance body.
data Tree a = Leaf a | Branch (Tree a) (Tree a)

instance Show a => Show (Tree a) where
    show (Leaf x)     = "Leaf " <> show x
    show (Branch l r) = "Branch (" <> show l <> ") (" <> show r <> ")"

-- The context must also propagate through an ordinary constrained function.
describe :: Show a => Tree a -> String
describe t = "tree: " <> show t

main :: IO ()
main = do
    assert (show (Branch (Leaf 1) (Leaf 2)) == "Branch (Leaf 1) (Leaf 2)")
        "constrained instance at Tree Int"
    -- Nested: the instance used at Tree (Tree Int) demands
    -- Show (Tree Int), which the instance itself provides.
    assert (show (Branch (Leaf (Leaf 1)) (Leaf (Leaf 2)))
            == "Branch (Leaf Leaf 1) (Leaf Leaf 2)")
        "constrained instance at Tree (Tree Int)"
    assert (show (Leaf "s") == "Leaf \"s\"") "constrained instance at Tree String"
    assert (describe (Branch (Leaf 3) (Leaf 4)) == "tree: Branch (Leaf 3) (Leaf 4)")
        "context through a constrained function"
