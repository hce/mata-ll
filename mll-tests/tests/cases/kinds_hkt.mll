-- Test: the kind system's positive side — higher-kinded classes and data.
--
-- A user-defined class whose variable is inferred at kind Type -> Type
-- (from the use `t Integer` in the method signature, no annotation), with
-- instances over the bare list constructor `[]`, over Maybe, and over the
-- partially applied `Either c`. Plus a data type with a higher-kinded
-- parameter (`Wrap f` gets kind (Type -> Type) -> Type from the field
-- `f Integer`), and a constrained function dispatching through all of it.

class Collapse t where
    collapse :: t Integer -> Integer

instance Collapse [] where
    collapse [] = 0
    collapse (x:xs) = x + collapse xs

instance Collapse Maybe where
    collapse Nothing = 0
    collapse (Just x) = x

instance Collapse (Either c) where
    collapse (Left _) = 0
    collapse (Right x) = x

data Wrap f = Wrap (f Integer)

unwrapCollapse :: Collapse f => Wrap f -> Integer
unwrapCollapse (Wrap fx) = collapse fx

-- The builtin Foldable/Traversable instances for [], Maybe and Either now
-- live in Prelude.mll as ordinary instance declarations (their heads are
-- kind-checked against Foldable's Type -> Type variable); exercise them
-- through the generic Prelude functions to prove dispatch still works.
main :: IO ()
main = do
    assert (collapse [1, 2, 3] == 6) "collapse list ([] instance head)"
    assert (collapse (Just 42) == 42) "collapse Maybe"
    assert (collapse (Right 7 :: Either String Integer) == 7) "collapse Either"
    assert (collapse (Left "no" :: Either String Integer) == 0) "collapse Left"
    assert (unwrapCollapse (Wrap [10, 20]) == 30) "higher-kinded Wrap []"
    assert (unwrapCollapse (Wrap (Just 5)) == 5) "higher-kinded Wrap Maybe"
    assert (sum [1, 2, 3] == 6) "Prelude Foldable [] via sum"
    assert (length (Just 1) == 1) "Prelude Foldable Maybe via length"
    assert (sum (Right 9 :: Either String Integer) == 9) "Prelude Foldable Either"
    case traverse (\x -> Just (x + 1)) [1, 2] of
        Just ys -> assert (ys == [2, 3]) "Prelude Traversable []"
        Nothing -> error "traverse: unexpected Nothing"
    putStrLn "All kind-system tests passed!"
-- expect: All kind-system tests passed!
