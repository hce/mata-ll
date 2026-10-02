import Data.Map (Map)
import qualified Data.Map as M
import Data.List (sortBy)

data Crate = Crate { size :: Int, tag :: String }

class Bag f where
    empty :: f
    insert :: Int -> f -> f
    count :: f -> Int

data IntBag = IntBag [Int]

instance Bag IntBag where
    empty = IntBag []
    insert x (IntBag xs) = IntBag (x : xs)
    count (IntBag xs) = length xs

area :: Int -> Int -> Int
area member keys = member * keys

pick :: (Int -> Bool) -> [Int] -> [Int]
pick find xs = filter find xs

table :: Map Int String
table = M.fromList [(1, "a"), (2, "b")]

descending :: [Int] -> [Int]
descending xs = sortBy (\a b -> compare b a) xs

main :: IO ()
main = do
    let elems = [3, 1, 2]
    let partition = length elems
    let c = Crate 3 "crate"
    assert (area 3 4 == 12) "parameters named like hidden imports"
    assert (pick even [1, 2, 3, 4] == [2, 4]) "a parameter named like a hidden import"
    assert (partition == 3) "a let named like a hidden import"
    assert (descending elems == [3, 2, 1]) "a let named like a hidden import, used"
    assert (size c == 3) "an own record field named like a hidden import"
    assert (count (insert 1 (empty :: IntBag)) == 1) "own class methods named like hidden imports"
    assert (M.size table == 2) "the alias still reaches the hidden names"
    assert (M.member 2 table) "the alias still reaches the hidden names"
    print (area 3 4, partition, size c, M.size table)
    print (descending elems)
