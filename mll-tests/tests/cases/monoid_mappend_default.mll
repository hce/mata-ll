-- A user Monoid instance defining only mempty gets GHC's default
-- `mappend = (<>)` from the Semigroup superclass (audit finding 16), and
-- first-class operator sections of class methods dispatch by instance
-- (the same repro's `foldr (<>) mempty` used to compile `<>` to Lua string
-- concatenation regardless of the type).
data Sum = Sum Integer deriving (Show, Eq)

instance Semigroup Sum where
    (<>) (Sum a) (Sum b) = Sum (a + b)

instance Monoid Sum where
    mempty = Sum 0

total :: [Sum] -> Sum
total = foldr (<>) mempty

check :: (Show a, Eq a) => String -> a -> a -> IO ()
check name got want =
    if got == want
        then putStrLn ("ok " <> name)
        else error ("FAIL " <> name <> ": got " <> show got <> " want " <> show want)

main :: IO ()
main = do
    check "defaulted-mappend" (mappend (Sum 4) (Sum 5)) (Sum 9)
    check "foldMap" (foldMap (\n -> Sum n) [1, 2, 3, 4]) (Sum 10)
    check "first-class-op" (total [Sum 1, Sum 2, Sum 3]) (Sum 6)
    check "mconcat" (mconcat [Sum 2, Sum 3, Sum 4]) (Sum 9)
    check "mconcat-string" (mconcat ["a", "b", "c"]) "abc"
