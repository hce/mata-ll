import Prelude hiding ((=<<), (<$))
import FixityPow

(=<<) :: Int -> Int -> Int
a =<< b = a - b

infixr 5 <$
(<$) :: Int -> Int -> Int
a <$ b = a - b

main :: IO ()
main = do
    assert (powTotal == 5) "an operator spelled like the Prelude's, without a declaration, is infixl 9 in its module"
    assert (powMixed == 23) "and one with a declaration uses its own"
    assert (2 ^ 3 ^ 2 == 512) "the Prelude's operator keeps the Prelude's fixity where it is the one in scope"
    assert (2 * 10 =<< 3 == 14) "the entry module's own operator without a declaration is infixl 9"
    assert (10 <$ 3 <$ 2 == 9) "and its declared one uses the declaration"
    assert (fmap (+ 1) (Just 1) == Just 2) "the Prelude's own code is unaffected"
    print (powTotal, powMixed, 2 ^ 3 ^ 2, 2 * 10 =<< 3, 10 <$ 3 <$ 2)
