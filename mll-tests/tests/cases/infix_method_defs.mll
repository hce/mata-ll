-- Infix definition form INSIDE class and instance bodies: `a <+> b =
-- …` (and the backtick spelling) define the operator with the
-- identifier as left operand, exactly as at the top level.  Regression:
-- class/instance method parsing only knew the prefix form, so the
-- spelling the top level accepts died with "Expected '='".

data V = V Int

class Joinable a where
    (<+>) :: a -> a -> a
    joinAll :: a -> a -> a
    x `joinAll` y = x <+> y

instance Joinable V where
    a <+> b = case (a, b) of
        (V x, V y) -> V (x + y)

instance Joinable Int where
    a <+> b = a + b + 100

showV :: V -> String
showV v = case v of
    V n -> "V " <> show n

main :: IO ()
main = do
    putStrLn (showV (V 1 <+> V 2))
    print ((3 :: Int) <+> 4)
    print (joinAll (5 :: Int) 6)
