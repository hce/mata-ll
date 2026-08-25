-- User instances at tuple heads: `instance (Pretty a, Pretty b) =>
-- Pretty (a, b)` is registered under its arity and its declared context
-- binds the element types positionally.  Regression: entailment failed
-- every non-structural class at tuples unconditionally, so a legal
-- tuple instance was unusable; and the context binder only peeled App
-- spines, so a tuple instance's context bound nothing.  Unit also
-- checks against its REGISTRY now (Show/Eq/Ord are registered; `Num ()`
-- is a compile error), pinned by the accept side here.

class Pretty a where
    pretty :: a -> String

instance Pretty Int where
    pretty n = "#" <> show n

instance Pretty Bool where
    pretty b = if b then "yes" else "no"

instance (Pretty a, Pretty b) => Pretty (a, b) where
    pretty p = case p of
        (x, y) -> "<" <> pretty x <> ", " <> pretty y <> ">"

main :: IO ()
main = do
    putStrLn (pretty (7 :: Int))
    putStrLn (pretty ((1, True) :: (Int, Bool)))
    -- the instance recurses through itself at a nested pair
    putStrLn (pretty (((2, False), 3) :: ((Int, Bool), Int)))
    -- Unit accept side: the registered builtin instances
    print ()
    print (() == ())
