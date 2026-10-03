import Data.IORef

keepLeft :: Int -> Int -> Int
keepLeft a _ = a

keepRight :: Int -> Int -> Int
keepRight _ b = b

add3 :: Int -> Int -> Int -> Int
add3 a b c = a * 100 + b * 10 + c

strictBoth :: Int -> Int -> Int
strictBoth a b = a + b

(<+) :: Int -> Int -> Int
a <+ _ = a

(+>) :: Int -> Int -> Int
a +> b = if a > 0 then b else b

infixr 5 <:>
(<:>) :: Int -> [Int] -> [Int]
x <:> xs = x : xs

record :: IORef [Int] -> Int -> IO ()
record r v = modifyIORef r (v :)

ones :: Int -> [Int]
ones x = x <:> ones x

main :: IO ()
main = do
    print (1 `keepLeft` error "right operand")
    print (error "left operand" `keepRight` 2)
    print ((`keepLeft` error "section operand") 3)
    print ((error "section operand" `keepRight`) 4)
    let n = length [1 :: Int]
    print ((n `add3` 2) 3)
    let partial = 5 `add3` 6
    print (map partial [7, 8])
    print (10 `strictBoth` 20)
    print (6 <+ error "symbolic right operand")
    print (length [error "symbolic left operand" +> 7 :: Int])
    print ((<+ error "symbolic section operand") 8)
    print (take 3 (ones 9))
    print (2 ^ (10 :: Int))
    log <- newIORef []
    let later = log `record` 30
    record log 40
    later
    later
    entries <- readIORef log
    print entries
    assert (entries == [30, 30, 40]) "a backtick action application is a value run where it is run"
