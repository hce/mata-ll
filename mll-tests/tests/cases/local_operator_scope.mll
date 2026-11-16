-- Signatures and fixity declarations are declarations of the group they
-- stand in (Haskell 2010 sections 4.4.1, 4.4.2): a where or let group's
-- `(<+>) :: T` and `infixl 6 <+>` belong to that group's `<+>`, which
-- shadows an outer operator of the same name with its own type and its
-- own fixity, in infix chains, sections and as a value. Two functions
-- each define their own `<+>`; a local function gets a fixity for its
-- backtick use; a local group redefines a Prelude operator. (F12i; these
-- declarations used to be hoisted to the top level, where the two `<+>`
-- collapsed into one.)
infixr 0 <+>
(<+>) :: Int -> Int -> Int
a <+> b = a - b

f :: Int -> Int
f n = n <+> 2 * 3
  where
    infixl 6 <+>
    (<+>) :: Int -> Int -> Int
    a <+> b = a * 100 + b

g :: Int -> Int
g n = n <+> 2 * 3
  where
    infixl 8 <+>
    (<+>) :: Int -> Int -> Int
    a <+> b = a * 100 + b

h :: Int -> Int
h n = n `plus` 2 * 3
  where
    infixl 8 `plus`
    a `plus` b = a * 100 + b

outer :: Int -> Int
outer n = n <+> 2 * 3

norm :: Int -> Int -> Int
norm a b = a <+> b + a `dot` b
  where
    infixl 6 <+>
    (<+>) :: Int -> Int -> Int
    x <+> y = x * 10 + y
    dot :: Int -> Int -> Int
    dot x y = x * y

scale :: Int -> [Int] -> [Int]
scale k xs =
  let (|>) :: Int -> Int -> Int
      a |> b = a * k + b
      infixr 5 |>
  in map (|> 1) xs ++ map (1 |>) xs ++ [foldr (|>) 0 xs, 1 |> 2 |> 3]

nested :: Int -> (Int, Int)
nested n = (n <+> 1 - 1, inner)
  where
    infixl 4 <+>
    a <+> b = a * 10 + b
    inner = n <+> 1 - 1
      where
        a <+> b = a * 1000 + b

shadowPrelude :: Int -> Int -> Int
shadowPrelude x y = x + y * 2
  where
    infixl 7 +
    a + b = a - b
    infixl 6 *
    a * b = a - b - 100

plus, times :: Int -> Int -> Int
plus a b = sum [a, b]
times a b = product [a, b]

differ :: Int -> Int -> Bool
differ a b = not (elem a [b])

shadowAll :: Int -> [Int] -> (Bool, [Int], Int, Int, Bool, Int, Bool, Int, Int)
shadowAll n xs =
  (n == 3, xs ++ [n], (n . 2) 5, n $ 4, True && False, xs !! 0, n < 1, n <> 2, n - 1 / 2 * 3)
  where
    a == b = differ a b
    as ++ bs = bs
    (g . h) x = times x 1000
    infixr 0 $
    a $ b = plus (times a b) 1
    a && b = a
    as !! i = length as
    a < b = True
    a <> b = plus (times a 7) b
    infixl 9 -
    a - b = plus (times a 10) b
    infixl 5 /
    a / b = plus (times a 100) b
    a * b = negate a

matching :: Maybe Int -> Maybe Int -> Int
matching a b = a <|> b <|> Just 0 `orZero` 5
  where
    infixr 3 <|>
    (<|>), keep :: Maybe Int -> Maybe Int -> Maybe Int
    Nothing <|> r = r
    l <|> _ = l
    keep l _ = l
    infixl 2 `orZero`
    Just v `orZero` d
      | v > 0 = v + d
      | otherwise = d
    Nothing `orZero` _ = 0

viaDo :: Int -> IO ()
viaDo n = do
  let infixl 6 |+|
      (|+|) :: Int -> Int -> Int
      a |+| b = a + b + n
      (lhs .*. rhs) c = lhs * rhs * c
  print (1 |+| 2 * 3, (2 .*. 3) 4)

main :: IO ()
main = do
  print (f 1, g 1, h 1, outer 1)
  print (norm 2 3)
  print (scale 2 [1, 2, 3])
  print (nested 1)
  print (shadowPrelude 10 3)
  print (matching Nothing (Just 3), matching (Just (-1)) Nothing, matching Nothing Nothing)
  viaDo 10
  print (shadowAll 3 [1, 2])
  print (let a >>= b = plus a b in 1 >>= 2, let a >> b = times a b in 3 >> 4, let (+) = times in 3 + 4)
  print (let x |> g = g x; infixl 1 |> in (3 :: Int) |> succ |> show)
  print (10 <+> 3 <+> 2)
