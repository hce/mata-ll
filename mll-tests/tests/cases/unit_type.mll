-- Regression tests: () must be a first-class (if trivial) type.
-- Covers the bug cluster where () was second-class: no Show/Eq/Ord
-- instances, rejected in pattern position, and its nil runtime rep
-- leaking through Maybe's show as "Just Nothing".

-- () as a case pattern (bare)
unitCase :: () -> Integer
unitCase u = case u of
  () -> 42

-- () as a function-clause pattern
unitClause :: () -> String
unitClause () = "matched"

-- () inside a constructor pattern
fromMaybeUnit :: Maybe () -> Integer
fromMaybeUnit x = case x of
  Just () -> 1
  Nothing -> 0

-- () nested two deep: Just Nothing / Just (Just ()) must stay distinct
nested :: Maybe (Maybe ()) -> Integer
nested m = case m of
  Just (Just ()) -> 2
  Just Nothing -> 1
  Nothing -> 0

-- () as an ADT field, with derived Show and Eq
data E = L () | R Integer deriving (Show, Eq)

pick :: E -> String
pick e = case e of
  L () -> "left"
  R n -> "right"

main :: IO ()
main = do
  -- Show (): the nil runtime rep must not render as "Nothing"
  assert (show () == "()") "show () should be ()"
  assert (show (Just ()) == "Just ()") "show (Just ()) should be Just ()"
  assert (show (Just (Just ())) == "Just (Just ())") "show nested Just ()"
  assert (show (Nothing :: Maybe ()) == "Nothing") "show Nothing at Maybe ()"
  assert (show [(), ()] == "[(),()]") "show list of units"
  assert (show (1 :: Integer, ()) == "(1,())") "show tuple with unit"
  assert (show (L ()) == "L ()") "derived Show with unit field"
  -- Eq/Ord ()
  assert (() == ()) "unit equals itself"
  assert (not (() /= ())) "unit not unequal to itself"
  assert (compare () () == EQ) "compare () () is EQ"
  assert (() <= ()) "() <= ()"
  assert (() >= ()) "() >= ()"
  assert (not (() < ())) "not () < ()"
  assert (not (() > ())) "not () > ()"
  assert (L () == L ()) "derived Eq with unit field (equal)"
  assert (L () /= R 1) "derived Eq with unit field (unequal)"
  -- () in pattern position
  assert (unitCase () == 42) "bare () case pattern"
  assert (unitClause () == "matched") "() function-clause pattern"
  assert (fromMaybeUnit (Just ()) == 1) "Just () pattern"
  assert (fromMaybeUnit Nothing == 0) "Nothing at Maybe ()"
  assert (nested (Just (Just ())) == 2) "Just (Just ()) pattern"
  assert (nested (Just Nothing) == 1) "Just Nothing pattern at Maybe (Maybe ())"
  assert (nested Nothing == 0) "Nothing pattern at Maybe (Maybe ())"
  assert (pick (L ()) == "left") "L () pattern"
  assert (pick (R 9) == "right") "R n pattern"
