-- Haskell 2010 §3.13 binding guard qualifiers: pattern guards
-- (`pat <- expr`, falling through on mismatch) and `let` qualifiers,
-- freely mixed with boolean qualifiers, in function clauses (falling
-- through to LATER CLAUSES), case alternatives, where bindings, and
-- instance methods. The parser lowers them to lazily let-bound join
-- points, so nothing is evaluated twice and untaken alternatives are
-- never evaluated at all.
import qualified Data.Map as M

-- The idiom that motivates the feature.
price :: M.Map String Int -> String -> String
price m k
    | Just v <- M.lookup k m, v > 100 = "expensive " <> k
    | Just v <- M.lookup k m = k <> " costs " <> show v
    | otherwise = "no " <> k

-- All three qualifier forms in one guard; fall-through to a later
-- guard, a sandwiched PLAIN clause, and a final plain clause.
classify :: Maybe Int -> Int -> String
classify m n
    | Just v <- m, v > n, let d = v - n = "above by " <> show d
    | Just v <- m = "at most " <> show n <> " (got " <> show v <> ")"
classify _ n | n > 10 = "nothing, big n"
classify _ _ = "nothing"

-- Literal and tuple patterns as qualifiers; a boolean guard STARTING
-- with a constructor expression must still parse as a boolean.
shapes :: (Int, Int) -> Maybe Int -> String
shapes p m
    | 0 <- fst p, (x, y) <- p, x <= y = "zero-first ordered"
    | Just 3 == m = "exactly three"
    | (x, y) <- p, let s = x + y, odd s = "odd sum " <> show s
    | otherwise = "other"

-- Case alternatives: pattern-guard branches fall through to later
-- branches, plain guards after them still work.
caseSide :: [Int] -> String
caseSide xs = case xs of
    (x:_) | even x, let h = x * 2 -> "even head doubled: " <> show h
          | x > 100 -> "big odd head"
    [x] | odd x -> "single odd"
    _ -> "rest"

-- Where bindings take the same qualifiers (single equation, error on
-- total fall-through), and the where scope reaches every guard.
whereSide :: Int -> String
whereSide n = go n
  where
    go k
        | Just v <- half k = "half " <> show v
        | otherwise = "odd " <> show k
    half k = if even k then Just (k `div` 2) else Nothing

-- Laziness: matching Just forces only the spine; the payload, the later
-- guards and the later clause (the fall-through join) stay unevaluated
-- when the first guard takes.
spineOnly :: Maybe Int -> String
spineOnly m
    | Just _ <- m = "got one"
    | error "untaken guard" = error "untaken body"
spineOnly _ = error "untaken clause"

data Wrap = Wrap (Maybe Int)

class Describe a where
    describe :: a -> String

instance Describe Wrap where
    describe w
        | Wrap (Just v) <- w, v >= 0 = "wrapped " <> show v
        | Wrap (Just v) <- w = "wrapped negative " <> show v
        | otherwise = "wrapped nothing"

-- Pattern guards nested inside a pattern-guard body's case.
nested :: [Maybe Int] -> String
nested l
    | (m:_) <- l = case m of
        v | Just x <- v, x > 1 -> "head just big " <> show x
          | Just x <- v -> "head just " <> show x
        _ -> "head nothing"
nested _ = "empty"

main :: IO ()
main = do
    let m = M.fromList [("tea", 3), ("gold", 999)]
    putStrLn (price m "tea")
    putStrLn (price m "gold")
    putStrLn (price m "ale")
    putStrLn (classify (Just 9) 4)
    putStrLn (classify (Just 3) 4)
    putStrLn (classify Nothing 20)
    putStrLn (classify Nothing 4)
    putStrLn (shapes (0, 2) Nothing)
    putStrLn (shapes (1, 2) (Just 3))
    putStrLn (shapes (1, 2) Nothing)
    putStrLn (shapes (2, 2) Nothing)
    putStrLn (caseSide [4, 5])
    putStrLn (caseSide [101])
    putStrLn (caseSide [7])
    putStrLn (caseSide [])
    putStrLn (whereSide 8)
    putStrLn (whereSide 7)
    putStrLn (spineOnly (Just (error "payload untouched")))
    putStrLn (describe (Wrap (Just 5)))
    putStrLn (describe (Wrap (Just (-5))))
    putStrLn (describe (Wrap Nothing))
    putStrLn (nested [Just 2])
    putStrLn (nested [Just 1])
    putStrLn (nested [Nothing])
    putStrLn (nested [])

-- expect: tea costs 3
-- expect: expensive gold
-- expect: no ale
-- expect: above by 5
-- expect: at most 4 (got 3)
-- expect: nothing, big n
-- expect: nothing
-- expect: zero-first ordered
-- expect: exactly three
-- expect: odd sum 3
-- expect: other
-- expect: even head doubled: 8
-- expect: big odd head
-- expect: single odd
-- expect: rest
-- expect: half 4
-- expect: odd 7
-- expect: got one
-- expect: wrapped 5
-- expect: wrapped negative -5
-- expect: wrapped nothing
-- expect: head just big 2
-- expect: head just 1
-- expect: head nothing
-- expect: empty
