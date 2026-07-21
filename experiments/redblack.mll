-- Red-black tree: a self-checking compiler stress test.
--
-- Exercises the deepest nested constructor patterns in the suite: Okasaki's
-- `balance` matches shapes like  T R (T R a x b) y c , which stress pattern-
-- match compilation and exhaustiveness checking. Also exercises guards,
-- recursive ADTs at many positions, and where-bound local recursion.
--
-- Oracles (no external check needed):
--   * in-order traversal is strictly sorted (BST property + dedup)
--   * the red-black invariants hold (no red-red, uniform black height)
--   * every inserted key is a member; a never-inserted key is not
-- A failed assert -> error -> the program (and its test) fails.

data Color = R | B

data Tree = E | T Color Tree Integer Tree

isBlack :: Color -> Bool
isBlack B = True
isBlack R = False

-- The four rotation cases plus the catch-all. This is the core stress: each
-- of the first four patterns is a doubly-nested constructor match.
balance :: Color -> Tree -> Integer -> Tree -> Tree
balance B (T R (T R a x b) y c) z d = T R (T B a x b) y (T B c z d)
balance B (T R a x (T R b y c)) z d = T R (T B a x b) y (T B c z d)
balance B a x (T R (T R b y c) z d) = T R (T B a x b) y (T B c z d)
balance B a x (T R b y (T R c z d)) = T R (T B a x b) y (T B c z d)
balance color a x b                 = T color a x b

insert :: Integer -> Tree -> Tree
insert x t = makeBlack (ins t)
  where
    ins E = T R E x E
    ins (T color a y b)
      | x < y     = balance color (ins a) y b
      | x > y     = balance color a y (ins b)
      | otherwise = T color a y b
    makeBlack (T _ a y b) = T B a y b
    makeBlack E           = E

member :: Integer -> Tree -> Bool
member _ E = False
member x (T _ a y b)
  | x < y     = member x a
  | x > y     = member x b
  | otherwise = True

fromList :: [Integer] -> Tree
fromList = foldl (\t x -> insert x t) E

toList :: Tree -> [Integer]
toList E           = []
toList (T _ a x b) = toList a ++ (x : toList b)

-- ── invariant checks ────────────────────────────────────────────────────

-- No red node has a red child. Note the nested patterns here too.
redOK :: Tree -> Bool
redOK E                       = True
redOK (T R (T R _ _ _) _ _)   = False
redOK (T R _ _ (T R _ _ _))   = False
redOK (T _ a _ b)             = redOK a && redOK b

-- Returns the black-height, or -1 if the two subtrees disagree (invariant
-- broken). Empty nodes count as black.
blackHeight :: Tree -> Integer
blackHeight E = 1
blackHeight (T c a _ b) =
  let lh = blackHeight a
      rh = blackHeight b
  in if lh < 0 || rh < 0 || not (lh == rh)
       then -1
       else lh + (if isBlack c then 1 else 0)

isSorted :: [Integer] -> Bool
isSorted (x : y : rest) = x < y && isSorted (y : rest)
isSorted _              = True

eqInts :: [Integer] -> [Integer] -> Bool
eqInts []     []     = True
eqInts (x:xs) (y:ys) = x == y && eqInts xs ys
eqInts _      _      = False

allMembers :: [Integer] -> Tree -> Bool
allMembers []     _ = True
allMembers (x:xs) t = member x t && allMembers xs t

valid :: Tree -> Bool
valid t = redOK t && blackHeight t > 0 && isSorted (toList t)

-- ── driver ──────────────────────────────────────────────────────────────

ascending :: [Integer]
ascending = enumFromTo 1 31

descending :: [Integer]
descending = reverse (enumFromTo 1 31)

-- duplicates and a jumbled order; distinct keys are exactly 1..9
jumbled :: [Integer]
jumbled = [5, 3, 8, 1, 4, 7, 9, 2, 6, 3, 5, 8, 1, 9]

main :: IO ()
main = do
  let tAsc  = fromList ascending
  let tDesc = fromList descending
  let tJmb  = fromList jumbled

  -- Worst-case insertion orders for a naive BST exercise the balance cases.
  assert (valid tAsc)  "redblack: ascending inserts stay balanced + sorted"
  assert (valid tDesc) "redblack: descending inserts stay balanced + sorted"
  assert (valid tJmb)  "redblack: jumbled-with-dups inserts valid"

  -- In-order traversal recovers the sorted, de-duplicated key set.
  assert (eqInts (toList tAsc) (enumFromTo 1 31)) "redblack: asc in-order == 1..31"
  assert (eqInts (toList tDesc) (enumFromTo 1 31)) "redblack: desc in-order == 1..31"
  assert (eqInts (toList tJmb) (enumFromTo 1 9))   "redblack: jumbled in-order == 1..9"

  -- Membership: everything inserted is found; a gap key is not.
  assert (allMembers jumbled tJmb) "redblack: all inserted keys are members"
  assert (not (member 100 tJmb))   "redblack: absent key is not a member"

  putStrLn ("ascending black-height:  " <> show (blackHeight tAsc))
  putStrLn ("jumbled distinct keys:   " <> show (length (toList tJmb)))
  putStrLn "all red-black tree checks passed"
