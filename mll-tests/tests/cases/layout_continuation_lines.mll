-- Pins the layout rule as one token-stream property (Haskell 2010
-- §10.3): a line indented past the enclosing block's column continues
-- the item above it, whatever construct is open. Every multi-line shape
-- here is written the way GHC accepts it: a signature's `::` and `->` on
-- their own lines, a clause's `=` on the next line, guards with `|` and
-- `=` on separate lines (in a clause, a where binding and a case
-- alternative), a case alternative's `->` on the next line, if/then/else
-- on three lines (also at a do block's statement column), let bindings
-- and lambda bodies continued, data/newtype declarations with `=`, `|`,
-- fields, record braces and `deriving` on their own lines, class and
-- instance heads with `=>` on the next line, a GADT's `deriving` at its
-- constructors' column, list and tuple literals spanning lines, sections
-- and infix operators (symbolic and backtick) at a line start, a record
-- brace on the next line, a `where` on its own line after a guard set, a
-- clause guard aligned with the alternatives of the case it follows, an
-- `in` aligned with its let's bindings, and record bodies whose `{`, `,`
-- and `}` stand at (or left of) the enclosing block's column — a brace
-- suspends layout until its match (Haskell 2010 §10.3's context 0). The
-- parser used to accept each shape only where a parse site had been
-- taught to skip the line start (F11).

sign
  :: Int
  -> Int
sign n
  | n < 0
  = -1
  | n == 0
  = 0
  | otherwise
  = 1

longName :: Int
longName
  = 5

classify :: Int -> String
classify n = case n of
  0
    -> "zero"
  _
    | n < 0
    -> "negative"
    | otherwise
    -> "positive"

pick :: Bool -> Int
pick b =
  if b
    then 1
    else 2

pickDo :: Bool -> IO Int
pickDo b = do
  r <- return b
  if r
  then return 10
  else return 20

addUp :: Int -> Int
addUp n =
  let a = n
        + 1
      b = a
        * 2
  in a + b

twice :: (Int -> Int) -> Int -> Int
twice f = \x ->
  f
    (f x)

newtype W
  = W Int

unW :: W -> Int
unW (W n) = n

data P = P
  Int
  Int
  deriving Show

data Q = Q Int
         Int
  deriving Show

data C = Red | Green
  deriving
    (Show, Eq)

data D = D Int
  deriving
  Show

data Shape
  = Circle Int
  | Rect
      Int
      Int
  deriving (Show, Eq)

data R = R
  { rx :: Int
  , ry :: Int
  }
  deriving Show

data G where
  GI :: Int -> G
  GB :: Bool -> G
  deriving Show

class Describe a
  where
  describe :: a -> String

instance Describe P
  where
  describe (P a b) = "P" <> show (a + b)

class (Show a)
  => Tagged a where
  tag :: a -> String
  tag x = "<" <> show x <> ">"

instance (Show a)
  => Tagged (Maybe a) where

pair :: (Int, Int) -> Int
pair (a, b) = a + b

guardedWhere :: Int -> Int
guardedWhere n
  | n > limit = limit
  | otherwise = n
  where
    limit = 100

localGuards :: Int -> Int
localGuards n = go n
  where
    go k
      | k > 10
      = 10
      | otherwise
      = k

afterCase :: Int -> Int
afterCase n
  | n > 0 = case n of
      1 -> 100
      _ -> 200
  | otherwise = 300

alignedGuard :: Int -> Int
alignedGuard n
  | n > 0 = case n of
    1 -> 100
    _ -> 200
  | otherwise = 300

alignedIn :: Int -> Int
alignedIn n =
  let x = n * 2
      y = x + 1
  in x + y

braceAtWhereColumn :: Int -> Int
braceAtWhereColumn n = rx r
  where
    r = R {
    rx = n
    , ry = 0
    }

alignedInDo :: IO Int
alignedInDo = do
  let x = 3 :: Int
      y = x + 1
  return (let z = y * 2
              w = z + 1
          in z + w)

main :: IO ()
main = do
  print (map sign [-5, 0, 7])
  print longName
  print (map classify [-1, 0, 1])
  print (pick True, pick False)
  a <- pickDo True
  b <- pickDo False
  print (a, b)
  print (addUp 3)
  print (twice (+ 1) 5)
  print (unW (W 42))
  print (P 1 2)
  print (Q 3 4)
  print [Red, Green]
  print (Red == Green)
  print (D 5)
  print [Circle 1, Rect 2 3]
  print (Rect 2 3 == Rect 2 3)
  print (R { rx = 1, ry = 2 })
  print (GI 7)
  print (GB True)
  putStrLn (describe (P 1 2))
  putStrLn (tag (Just (3 :: Int)))
  print (pair (1, 2))
  print (guardedWhere 500, guardedWhere 5)
  print (localGuards 50, localGuards 5)
  print (afterCase 1, afterCase 2, afterCase (-1))
  print (alignedGuard 1, alignedGuard 2, alignedGuard (-1))
  print (alignedIn 4)
  r <- alignedInDo
  print r
  let xs =
        [ 1
        , 2
        , 3 :: Int
        ]
  print xs
  let tup =
        ( 1 :: Int
        , "two"
        )
  print tup
  let inc = (+
          1)
  print (inc (1 :: Int))
  let total = 1
        + 2
        * 3
        - 4 :: Int
  print total
  let composed = negate
        . (* 2)
        $ 5 :: Int
  print composed
  let halved = (10 :: Int)
        `div` 3
  print halved
  let bound = [1, 2 :: Int]
        >>= \i ->
          [i, i * 10]
  print bound
  let rec0 = R { rx = 0, ry = 0 }
  let rec1 = rec0
        { rx = 9 }
  print rec1
  let built = R
        { rx = 5
        , ry = 6
        }
  print built
  let request = R {
        rx = 1
      , ry = 2
      }
  print request
  let updated = request { rx = 2
      , ry = 3 }
  print updated
  print request {
  rx = 9
  }
  print (braceAtWhereColumn 4)
