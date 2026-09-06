-- One module imported through an unqualified list AND two qualified
-- aliases (B7). Module resolution used to COPY the declarations once per
-- alias, prefixed, so the data type existed three times with the same
-- constructors ("Duplicate data constructor"), `Shape` and `D.Shape` were
-- different types, and an instance on a builtin type was declared three
-- times. Now every origin module is merged exactly once and each alias
-- resolves to that copy — including qualified constructors (`D.Circle`
-- in expressions and patterns) and qualified class methods / record
-- fields, which GHC allows.
import DualForms (Shape(..), area)
import qualified DualForms as D
import qualified DualForms as E

-- The same type under all three spellings.
same :: Shape -> D.Shape
same s = s

viaE :: E.Shape -> Number
viaE = D.area

kind :: D.Shape -> String
kind (D.Circle _) = "circle"
kind (E.Rect _ _) = "rect"

main :: IO ()
main = do
    assert (area (Circle 2) == 12) "unqualified function on unqualified constructor"
    assert (D.area (same (Rect 2 3)) == 6) "alias function on the identical type"
    assert (viaE (Circle 1) == 3) "second alias names the same type"
    assert (show (Rect 1 1) == "shape of area 1.0") "Show instance merged once"
    assert (Rect 1 1 == E.Rect 1 1) "Eq instance; qualified constructor in an expression"
    assert (D.scale 2 == 20) "alias reaches a name the unqualified list hid"
    assert (weight (D.Tag { tagName = "a", tagWeight = 9 }) == 9) "qualified record construction"
    assert (D.weight (3 :: Int) == 3) "qualified class method; instance on Int merged once"
    assert (D.tagName (Tag "b" 1) == "b") "qualified record field"
    assert (kind (D.Circle 3) == "circle") "qualified constructor pattern"
    assert (kind (Rect 1 2) == "rect") "second alias in a pattern"
    assert (filter even [1, 2, 3, 4] == [2, 4]) "a private name that is also the Prelude's stays the Prelude's"
    putStrLn "import dual forms ok"
