-- Regression: tuple equality whose elements are user ADTs with derived Eq.
-- The generated tuple eq references its element eq functions inside a
-- SpecCall spec string ("__mll_tuple_eq:2:eq_Foo,eq_Bar"); dead-code
-- elimination used to split that string only on ':' and never saw the
-- comma-joined names, so eq_Foo/eq_Bar were dropped as dead and the program
-- crashed calling nil.

data Foo = MkFoo | OtherFoo deriving (Show, Eq)
data Bar = MkBar | OtherBar deriving (Show, Eq)

main :: IO ()
main = do
    assert ((MkFoo, MkBar) == (MkFoo, MkBar)) "eq equal tuple"
    assert ((MkFoo, MkBar) /= (MkFoo, OtherBar)) "neq second elem"
    assert ((MkFoo, MkBar) /= (OtherFoo, MkBar)) "neq first elem"
    -- Three elements exercise the comma list with more than one separator.
    assert ((MkFoo, MkBar, MkFoo) == (MkFoo, MkBar, MkFoo)) "eq triple"
    putStrLn "ok"
