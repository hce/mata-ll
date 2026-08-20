-- `f $ x` is an application and must stay lazy in lazy positions.
-- Regression: `$` sits in is_builtin_op (for emission dispatch), and the
-- cheapness walk treated any `lhs $ rhs` with cheap operands as "cheap to
-- force" — so an unused where-binding `u = boom $ 0` was assigned
-- STRICTLY and evaluated the application eagerly, raising (or looping)
-- where GHC never touches the binding. Same for a lazily-used argument
-- `lazyFst 1 (boom $ 2)`.
-- The one genuinely cheap `$` shape — a constructor applied through `$`,
-- which only builds a table — keeps working with lazy fields.

boom :: Int -> Int
boom _ = error "boom"

-- unused where-binding: must never be evaluated
g :: Int -> Int
g k = k + 1
  where unused = boom $ 0

-- unused argument: must never be evaluated
lazyFst :: Int -> Int -> Int
lazyFst a _ = a

-- composition only builds a closure; an unused composed binding is safe
h :: Int -> Int
h k = k + 2
  where unusedComp = boom . boom

-- constructor through `$`: fields stay lazy
isJust' :: Maybe Int -> String
isJust' (Just _) = "just"
isJust' Nothing  = "nothing"

main :: IO ()
main = do
    print (g 41)
    print (lazyFst 1 (boom $ 2))
    print (h 40)
    putStrLn (isJust' (Just $ boom 7))
