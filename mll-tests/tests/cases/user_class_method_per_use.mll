-- A user-class method whose signature has type variables OUTSIDE the class
-- head must be instantiated freshly at EVERY occurrence (audit finding 13).
-- The method scheme used to quantify only the class variable, so the
-- element variables a/b were shared across a whole definition and using
-- `myfmap` at two element types in one do-block failed with a false
-- "Cannot unify Int with Bool". Built-in fmap was unaffected.
class MyFunctor f where
    myfmap :: (a -> b) -> f a -> f b

instance MyFunctor Maybe where
    myfmap _ Nothing  = Nothing
    myfmap f (Just a) = Just (f a)

data Box x = Box x deriving Show

instance MyFunctor Box where
    myfmap f (Box x) = Box (f x)

check :: (Show a, Eq a) => String -> a -> a -> IO ()
check name got want =
    if got == want
        then putStrLn ("ok " <> name)
        else error ("FAIL " <> name <> ": got " <> show got <> " want " <> show want)

main :: IO ()
main = do
    -- Two element types through ONE instance in one definition.
    check "int" (myfmap (+ 1) (Just (1 :: Int))) (Just 2)
    check "bool" (myfmap not (Just True)) (Just False)
    -- A third occurrence changing BOTH sides of the arrow.
    check "int-to-string" (myfmap show (Just (7 :: Int))) (Just "7")
    -- And a second instance in the same definition too.
    check "box" (case myfmap (* 3) (Box (2 :: Int)) of Box n -> n) 6
