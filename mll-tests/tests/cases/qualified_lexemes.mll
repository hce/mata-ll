-- Qualified names are lexemes (Haskell 2010 section 2.4): a constructor
-- name, a dot and a name with no space between them form ONE token, for
-- variables (Q.plus), constructors and types (Q.Circle, Q.Shape) and
-- operators (Q.<+>, P.., P.+). Pins every position such a token may stand
-- in — expression atoms, infix chains, both sections, first-class
-- operators, backticks, patterns, record construction, types — with the
-- fixity of a qualified operator coming from its DEFINING module, a module
-- alias that is also a local data constructor (separate namespaces, as in
-- GHC), a qualified Prelude alias, and the dot spellings around it that
-- stay composition, field access, ranges and literals. (F10)
import Prelude
import qualified Prelude as P
import QualOps (Pair(..), (<+>), plus)
import qualified QualOps as Q

data Mode = Q | N deriving Show

tag :: Mode -> Int
tag Q = 1
tag N = 2

describe :: Q.Shape -> String
describe (Q.Circle r) = "circle " <> show r
describe (Q.Rect w h) = "rect " <> show w <> "x" <> show h

classify :: Q.Shape -> Int
classify s = case s of
  Q.Circle _ -> 0
  Q.Rect _ _ -> 1

swap :: Q.Pair -> Q.Pair
swap (Q.Pair a b) = Q.Pair { first = b, second = a }

half :: Int -> Int
half n = n `div` 2

data Color = Red | Green | Blue deriving (Show, Eq, Ord, Enum, Bounded)

main :: IO ()
main = do
  print (Q.plus 1 2, Q.minus 10 3)
  print (3 `Q.plus` 4)
  print (10 `Q.minus` 3 `Q.minus` 2)
  print (map (`Q.plus` 1) [1, 2])
  print (map (10 `Q.minus`) [1, 2])
  print (1 Q.<+> 2 Q.<+> 3)
  print (True Q.<&&> False Q.<&&> True)
  print (map (Q.<+> 1) [1, 2])
  print (map (5 Q.<+>) [1, 2])
  print ((Q.<+>) 7 8)
  print (1 Q.<+> 2 * 3)
  print (False Q.<&&> True || True)
  print (1 Q.<+> 2 `Q.plus` 3)
  print (1 <+> 2 `plus` 3)
  print (tag Q, tag N, Q.plus (tag Q) 1)
  print (describe (Q.Circle 2), describe (Q.Rect 3 4))
  print (classify (Q.Circle 1), classify (Q.Rect 1 1))
  print (Q.area (Q.Rect 3 4), map Q.area [Q.Circle 1, Q.Rect 2 5])
  print (swap (Q.Pair 1 2))
  let p = Q.Pair { first = 7, second = 9 }
  print (p.first + p.second, (swap p).first)
  print (P.map (P.+ 1) [1, 2 :: P.Int])
  print (P.map (1 P.+) [1, 2 :: P.Int])
  print ((P.show P.. P.negate) (5 :: Int))
  print ((P.+) 2 3 :: Int, (P..) P.show P.negate (6 :: Int))
  print (map (Just . half) [4], map (Just . Q.plus 1) [1])
  print (((Just .) . Q.plus) 1 2, ((Just.) negate) 1)
  print ((. half) Just 8, (half .) negate 8)
  print ([Red ..], [Red .. Blue], [minBound .. maxBound :: Color])
  print (1.5 :: Number, 2 :: Int)
