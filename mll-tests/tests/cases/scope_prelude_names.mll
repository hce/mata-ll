import Prelude hiding (head)
import ScopeTake

head :: [a] -> a
head xs = last xs

main :: IO ()
main = do
    assert (upTo 3 [1, 2, 3, 4, 5] == [1, 2, 3]) "the module's own takeWhile"
    assert (span (\x -> x < 3) [1, 2, 3, 4, 5] == ([1, 2], [3, 4, 5])) "span keeps the Prelude's takeWhile"
    assert (firstOf [1, 2, 3] == 1) "the module's head is the Prelude's"
    assert (head [1, 2, 3] == 3) "the root's head is its own"
    print (upTo 3 [1, 2, 3, 4, 5])
    print (span (\x -> x < 3) [1, 2, 3, 4, 5])
    print (firstOf [1, 2, 3], head [1, 2, 3])
