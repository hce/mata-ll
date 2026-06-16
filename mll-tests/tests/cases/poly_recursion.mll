data Nested a = NNil | NCons a (Nested [a])

depth :: Nested a -> Integer
depth NNil = 0
depth (NCons _ rest) = 1 + depth rest

showNested :: Show a => Nested a -> String
showNested NNil = "Nil"
showNested (NCons x rest) = "Cons " <> show x <> " (" <> showNested rest <> ")"

-- Deep nesting test: each level wraps in Box, creating polymorphic recursion.
-- showDeep calls itself at progressively different types:
--   Deep Integer -> Deep (Box Integer) -> Deep (Box (Box Integer)) -> ...
data Box a = Box a
    deriving (Show, Eq)

data Deep a = DNil | DCons a (Deep (Box a))

depthD :: Deep a -> Integer
depthD DNil = 0
depthD (DCons _ rest) = 1 + depthD rest

main :: IO ()
main = do
    let n = NCons 1 (NCons [2,3] (NCons [[4,5],[6]] NNil))
    assert (depth n == 3) "depth 3-deep nested"
    assert (depth NNil == 0) "depth Nil"
    assert (showNested n == "Cons 1 (Cons [2, 3] (Cons [[4, 5], [6]] (Nil)))") "showNested"
    let d = DCons 1 (DCons (Box 1) (DCons (Box (Box 1)) DNil))
    assert (depthD d == 3) "depthD 3-deep Box chain"
    assert (depthD DNil == 0) "depthD Nil"
