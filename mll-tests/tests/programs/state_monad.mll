-- A hand-rolled State monad: newtype over a function with Functor,
-- Applicative and Monad instances, used to label a tree, drive a stack
-- machine, and pull numbers from a linear congruential generator.
-- Leans on: user instances of the builtin Functor/Applicative/Monad
-- classes at a partially applied type constructor, do-notation and
-- mapM/mapM_/when over a user monad, deriving Functor on a tree.
import Control.Monad (when, unless)

newtype State s a = State (s -> (a, s))

runState :: State s a -> s -> (a, s)
runState (State f) = f

evalState :: State s a -> s -> a
evalState m s = fst (runState m s)

execState :: State s a -> s -> s
execState m s = snd (runState m s)

instance Functor (State s) where
    fmap f (State g) = State (\s -> case g s of (a, s') -> (f a, s'))

instance Applicative (State s) where
    pure a = State (\s -> (a, s))
    (<*>) (State f) (State g) = State (\s -> case f s of
        (h, s1) -> case g s1 of
            (a, s2) -> (h a, s2))
    liftA2 h (State f) (State g) = State (\s -> case f s of
        (a, s1) -> case g s1 of
            (b, s2) -> (h a b, s2))

instance Monad (State s) where
    (>>=) (State g) f = State (\s -> case g s of (a, s') -> runState (f a) s')
    (>>) m k = m >>= \_ -> k
    return = pure

get :: State s s
get = State (\s -> (s, s))

put :: s -> State s ()
put s = State (\_ -> ((), s))

modify :: (s -> s) -> State s ()
modify f = State (\s -> ((), f s))

-- Fresh labels for a tree.
data Tree a = Leaf | Node (Tree a) a (Tree a)
    deriving (Show, Functor)

fresh :: State Int Int
fresh = do
    n <- get
    put (n + 1)
    pure n

label :: Tree a -> State Int (Tree (Int, a))
label Leaf = pure Leaf
label (Node l x r) = do
    l' <- label l
    n <- fresh
    r' <- label r
    pure (Node l' (n, x) r')

toList :: Tree a -> [a]
toList Leaf = []
toList (Node l x r) = toList l ++ [x] ++ toList r

sample :: Tree String
sample = Node (Node Leaf "a" (Node Leaf "b" Leaf)) "c" (Node (Node Leaf "d" Leaf) "e" Leaf)

-- A stack machine whose state is the stack.
data Instr = Push Integer | Add | Mul | Dup | Swap | Pop
    deriving Show

push :: Integer -> State [Integer] ()
push x = modify (\st -> x : st)

pop :: State [Integer] Integer
pop = do
    st <- get
    case st of
        [] -> pure 0
        (x : rest) -> do
            put rest
            pure x

exec :: Instr -> State [Integer] ()
exec (Push x) = push x
exec Add = do
    a <- pop
    b <- pop
    push (a + b)
exec Mul = do
    a <- pop
    b <- pop
    push (a * b)
exec Dup = do
    a <- pop
    push a
    push a
exec Swap = do
    a <- pop
    b <- pop
    push a
    push b
exec Pop = do
    _ <- pop
    pure ()

runMachine :: [Instr] -> [Integer]
runMachine prog = execState (mapM_ exec prog) []

-- Linear congruential generator threaded through the state.
nextRand :: State Integer Integer
nextRand = do
    seed <- get
    let seed' = (seed * 1103515245 + 12345) `mod` 2147483648
    put seed'
    pure (seed' `div` 65536 `mod` 100)

randoms :: Int -> State Integer [Integer]
randoms n = mapM (\_ -> nextRand) [1 .. n]

-- Count how many draws it takes to see a value below 10, using when/unless.
drawUntilSmall :: State Integer Int
drawUntilSmall = go 0
  where
    go count = do
        r <- nextRand
        if r < 10 then pure (count + 1) else go (count + 1)

countdown :: State Int [Int]
countdown = do
    n <- get
    when (n > 0) (put (n - 1))
    unless (n > 0) (put 100)
    m <- get
    if n > 0 then fmap (\rest -> m : rest) countdown else pure [m]

main :: IO ()
main = do
    let (labelled, next) = runState (label sample) 0
    print labelled
    print (toList labelled)
    putStrLn ("next label: " <> show next)
    print (fmap (\(n, s) -> s <> show n) labelled)
    print (runMachine [Push 2, Push 3, Add, Dup, Mul, Push 7, Swap, Pop])
    print (runMachine [Add, Push 1])
    print (evalState (randoms 8) 42)
    print (evalState (randoms 3) 42 == take 3 (evalState (randoms 8) 42))
    print (runState drawUntilSmall 7)
    print (evalState countdown 5)
    print (execState (mapM_ (\x -> modify (+ x)) [1 .. 100]) 0)
    print (evalState (liftA2 (\a b -> (a, b)) fresh fresh) 10)
    print (evalState ((\a b -> a * 10 + b) <$> fresh <*> fresh) 3)
    print (evalState (fresh >> fresh >> fresh) 0)
