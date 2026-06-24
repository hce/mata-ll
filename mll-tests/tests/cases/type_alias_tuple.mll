-- Regression test: a parametric `type` alias whose body wraps a tuple must
-- have its parameter substituted *inside* the tuple. Before the fix,
-- substitute_type fell through to a catch-all for Type::Tuple, so the alias
-- parameter leaked through unsubstituted; same-named leaks from separate
-- expansions then collided and unification reported a bogus
-- "Infinite type: a occurs in (b, a)".

-- An alias wrapping a tuple of (value, remaining-input) -- the classic
-- parser-combinator shape that triggered the bug.
type PR a = Either String (a, [Integer])

andThen :: PR a -> (a -> [Integer] -> PR b) -> PR b
andThen (Left e) _ = Left e
andThen (Right (a, ts)) f = f a ts

-- Pull one positive integer off the front of the input.
takeNum :: [Integer] -> PR Integer
takeNum [] = Left "empty"
takeNum (x:rest) = if x == 0 then Left "zero" else Right (x, rest)

-- Nested binds whose inner result builds a *tuple* (a, b): this is the exact
-- combination (tuple-wrapping alias + nesting + tuple result) that failed.
pair :: [Integer] -> PR (Integer, Integer)
pair ts = takeNum ts `andThen` \a ts1 -> takeNum ts1 `andThen` \b ts2 -> Right ((a, b), ts2)

-- The same polymorphic combinator instantiated at a different result type,
-- to exercise multi-instantiation through the aliased signature.
triple :: [Integer] -> PR (Integer, Integer, Integer)
triple ts =
    takeNum ts `andThen` \a ts1 ->
    takeNum ts1 `andThen` \b ts2 ->
    takeNum ts2 `andThen` \c ts3 -> Right ((a, b, c), ts3)

fstOf :: PR (Integer, Integer) -> Integer
fstOf (Right ((a, _), _)) = a
fstOf (Left _) = 0 - 1

sndOf :: PR (Integer, Integer) -> Integer
sndOf (Right ((_, b), _)) = b
sndOf (Left _) = 0 - 1

restLen :: PR (Integer, Integer) -> Integer
restLen (Right (_, rest)) = length rest
restLen (Left _) = 0 - 1

tripleSum :: PR (Integer, Integer, Integer) -> Integer
tripleSum (Right ((a, b, c), _)) = a + b + c
tripleSum (Left _) = 0 - 1

main :: IO ()
main = do
    let p = pair [3, 4, 5]
    assert (fstOf p == 3) "pair first"
    assert (sndOf p == 4) "pair second"
    assert (restLen p == 1) "pair leftover"
    assert (tripleSum (triple [10, 20, 30]) == 60) "triple sum"
    case pair [0, 9] of
        Left e  -> assert (e == "zero") "error propagates"
        Right _ -> assert False "expected a Left"
    putStrLn "type_alias_tuple ok"
