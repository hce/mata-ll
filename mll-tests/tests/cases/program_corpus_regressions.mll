-- Regressions found by the differential program corpus (tests/programs),
-- one probe per fix, each asserting GHC's answer:
--   * a data declaration whose `=` / `|` start on continuation lines;
--   * a case alternative whose guard chain starts on the next line, and
--     the enclosing clause's own guard resuming after it;
--   * `<$>` dispatching to a user Functor instance (GHC's `(<$>) = fmap`);
--   * a user Monad instance that writes only `>>=` (GHC's `>>`/`return`
--     defaults), and a user Applicative that writes `liftA2` but not `<*>`;
--   * Foldable/Monad-generic calls (forM_, when) inside `runST (do …)`
--     — the ST instances demand nothing of the state token;
--   * the Prelude's `subtract`;
--   * a newtype-erased nested lambda (`\_ -> pure ()` in a State monad,
--     with `pure` inlined) keeping the arity its type declares.
import Control.Monad (forM_, when)

data Shape
    = Circle Int
    | Rect Int Int
    | Empty
    deriving (Show, Eq)

classify :: Maybe Int -> Int -> String
classify m k
    | k < 0 = "negative k"
    | otherwise = case m of
        Nothing -> "none"
        Just a
            | a < 0 -> "negative"
            | a == 0 -> "zero"
            | otherwise -> "positive"
    -- The guard below belongs to `classify`, not to the `Just a` alternative.

after :: Maybe Int -> String
after m
    | isJustNeg m = case m of
        Just a
            | a < -10 -> "very negative"
        _ -> "negative"
    | otherwise = "other"
  where
    isJustNeg (Just a) = a < 0
    isJustNeg Nothing = False

newtype Box a = Box a
    deriving Show

instance Functor Box where
    fmap f (Box a) = Box (f a)

newtype State s a = State (s -> (a, s))

runState :: State s a -> s -> (a, s)
runState (State f) = f

instance Functor (State s) where
    fmap f (State g) = State (\s -> case g s of (a, s') -> (f a, s'))

instance Applicative (State s) where
    pure a = State (\s -> (a, s))
    liftA2 h (State f) (State g) = State (\s -> case f s of
        (a, s1) -> case g s1 of
            (b, s2) -> (h a b, s2))

instance Monad (State s) where
    (>>=) (State g) f = State (\s -> case g s of (a, s') -> runState (f a) s')

tick :: State Int ()
tick = State (\s -> ((), s + 1))

-- `mapM_` is foldr over `>>` with `pure ()`: the `>>` default and the
-- inlined `pure` behind `\_ -> …` are both on this path.
ticks :: Int -> Int
ticks n = snd (runState (mapM_ (\_ -> tick) [1 .. n]) 0)

sumST :: Int -> Int
sumST n = runST (do
    arr <- newSTArray 1 0
    forM_ [1 .. n] (\i -> when (odd i) (modifySTArray arr 0 (+ i)))
    readSTArray arr 0)

main :: IO ()
main = do
    assert (show [Circle 1, Rect 2 3, Empty] == "[Circle 1,Rect 2 3,Empty]") "data decl with = and | on continuation lines"
    assert (map (classify (Just 5)) [1, -1] == ["positive", "negative k"]) "case-alt guards on the next line"
    assert (map (\m -> classify m 1) [Just (-2), Just 0, Nothing] == ["negative", "zero", "none"]) "guard chain of a case alternative"
    assert (map after [Just (-20), Just (-1), Just 3, Nothing] == ["very negative", "negative", "other", "other"]) "enclosing guard resumes after a guarded alternative"
    assert (show ((+ 1) <$> Box 41) == "Box 42") "<$> on a user Functor"
    assert (ticks 5 == 5) "user Monad with only >>= (>> default) and inlined pure behind a lambda"
    assert (fst (runState (return 7 >> pure 8) 0) == (8 :: Int)) "return default and >> default"
    assert (fst (runState ((\a b -> a * 10 + b) <$> pure 4 <*> pure 2) 0) == (42 :: Int)) "<*> default from liftA2"
    assert (sumST 10 == 25) "forM_/when inside runST (do …)"
    assert (map (subtract 3) [10, 3] == [7, 0]) "Prelude subtract"
    putStrLn "program_corpus_regressions: all assertions passed"
