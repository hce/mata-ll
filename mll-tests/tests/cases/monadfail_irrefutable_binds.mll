newtype W = W Int
data P = P Int Int
data Pair a b = Pair a b
data Box = forall a. Show a => Box a

tup :: Monad m => m (Int, Int) -> m Int
tup e = do
    (a, b) <- e
    return (a + b)

unit :: Monad m => m () -> m Int
unit e = do
    () <- e
    return 1

nt :: Monad m => m W -> m Int
nt e = do
    W a <- e
    return a

prod :: Monad m => m P -> m Int
prod e = do
    P a (b) <- e
    return (a + b)

deep :: Monad m => m (Pair Int (W, Int)) -> m Int
deep e = do
    Pair a (W b, c) <- e
    return (a + b + c)

named :: Monad m => m (Int, Int) -> m (Int, Int)
named e = do
    p@(a, _) <- e
    return (fst p + a, snd p)

wild :: Monad m => m (W, P) -> m Int
wild e = do
    (W _, P x _) <- e
    return x

boxed :: Monad m => m Box -> m String
boxed e = do
    Box v <- e
    return (show v)

main :: IO ()
main = do
    print (tup (Just (1, 2)))
    print (tup [(1, 2), (3, 4)])
    print (tup (Right (5, 6) :: Either String (Int, Int)))
    print (unit [(), ()])
    print (nt [W 5])
    print (nt (Left "no" :: Either String W) == Left "no")
    print (prod (Just (P 1 2)))
    print (deep (Just (Pair 1 (W 2, 3))))
    print (named (Just (3, 4)))
    print (wild [(W 1, P 9 0)])
    print (boxed (Just (Box 12)))
    r <- tup (return (10, 20))
    print r
    s <- deep (return (Pair 1 (W 2, 3)))
    print s
