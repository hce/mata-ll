import ScopeStack (Stack, empty, push, size)
import qualified Data.Map as M

depth :: Stack -> Int
depth s = size s

main :: IO ()
main = do
    assert (depth (push 1 (push 2 empty)) == 2) "the listed module's names"
    assert (M.size (M.insert (1 :: Int) "a" M.empty) == 1) "the alias's names"
    print (depth (push 1 empty), M.size (M.insert (1 :: Int) "a" M.empty))
