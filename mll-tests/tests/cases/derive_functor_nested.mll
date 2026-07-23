-- Derived Functor must recurse STRUCTURALLY (audit finding 14): the mapped
-- function has to reach every occurrence of the class variable, however
-- deeply nested. The old derivation mapped exactly one container level, so
-- `Maybe [a]` applied f to the inner LIST (arithmetic on a table) and a
-- self-recursive `[Rose a]` field applied f to whole subtrees.
data T a = T (Maybe [a]) deriving (Show, Eq, Functor)

data Rose a = Rose a [Rose a] deriving (Show, Eq, Functor)

data P a = P (Int, [a]) deriving (Show, Eq, Functor)

data G a = G (Int -> a) deriving (Functor)

apG :: G a -> Int -> a
apG (G g) n = g n

check :: (Show a, Eq a) => String -> a -> a -> IO ()
check name got want =
    if got == want
        then putStrLn ("ok " <> name)
        else error ("FAIL " <> name <> ": got " <> show got <> " want " <> show want)

main :: IO ()
main = do
    check "maybe-list" (fmap (* 2) (T (Just [3, 4]))) (T (Just [6, 8]))
    check "maybe-nothing" (fmap (* 2) (T Nothing)) (T Nothing)
    let r = Rose (1 :: Int) [Rose 2 [], Rose 3 [Rose 4 []]]
    check "rose" (fmap (* 2) r) (Rose 2 [Rose 4 [], Rose 6 [Rose 8 []]])
    check "tuple-list" (fmap (+ 1) (P (9, [1, 2]))) (P (9, [2, 3]))
    -- Covariant function field: fmap post-composes.
    check "covariant-fn" (apG (fmap (* 10) (G (\n -> n + 1))) 4) 50
