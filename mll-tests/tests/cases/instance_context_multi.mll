-- Regression: multi-constraint instance contexts. `(Show a, Show b) =>`
-- constrains two head variables at once, and `(Show a, Eq a) =>` lets one
-- method body use methods of TWO classes on the same instance variable —
-- including a context class (Show) different from the class being defined
-- (Eq).
data Pair a b = Pair a b

instance (Show a, Show b) => Show (Pair a b) where
    show (Pair x y) = "Pair " <> show x <> " " <> show y

data Tagged a = Tagged a

instance (Show a, Eq a) => Eq (Tagged a) where
    (==) (Tagged x) (Tagged y) =
        if x == y then True else error ("unequal: " <> show x)

main :: IO ()
main = do
    assert (show (Pair 1 "hi") == "Pair 1 \"hi\"") "two-variable context"
    assert (show (Pair True (Pair 2 3)) == "Pair True Pair 2 3")
        "two-variable context, nested"
    assert (Tagged 3 == Tagged 3) "Show+Eq context on one variable"
