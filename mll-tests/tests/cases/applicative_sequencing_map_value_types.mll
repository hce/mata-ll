-- `*>` and `<*` are Applicative class methods with base's defaults and
-- fixity (infixl 4): at every builtin instance ([], Maybe, Either, IO with
-- its effects in order, ST), at a user instance giving only pure/<*> or
-- only pure/liftA2 (the defaults), at one giving its own `*>` (which
-- wins), first-class, and through a dictionary (an existential packing
-- the Applicative evidence: both operators at the builtin instances, a
-- user instance's own `*>`, a liftA2-defined instance's default `<*`).
-- Data.Map.intersection/difference take two value types, as containers'
-- do; Data.Set's are the one-type controls.
import qualified Data.Map as M
import qualified Data.Set as S

newtype Id a = Id a deriving Show

instance Functor Id where
    fmap f (Id x) = Id (f x)

instance Applicative Id where
    pure x = Id x
    (Id f) <*> (Id x) = Id (f x)

data Two a = Two a a deriving Show

instance Functor Two where
    fmap f (Two x y) = Two (f x) (f y)

instance Applicative Two where
    pure x = Two x x
    liftA2 f (Two a b) (Two c d) = Two (f a c) (f b d)

data Tagged a = Tagged String a deriving Show

instance Functor Tagged where
    fmap f (Tagged t x) = Tagged t (f x)

instance Applicative Tagged where
    pure x = Tagged "" x
    (Tagged s f) <*> (Tagged t x) = Tagged (s <> t) (f x)
    (Tagged s _) *> (Tagged t y) = Tagged (s <> "|" <> t) y

data SomeApp = forall f. Applicative f => SomeApp (f Int -> String) (f Int) (f Int)

viaDict :: SomeApp -> String
viaDict (SomeApp render a b) = render (a *> b) <> " " <> render (a <* b)

viaThen :: SomeApp -> String
viaThen (SomeApp render a b) = render (a *> b)

viaBefore :: SomeApp -> String
viaBefore (SomeApp render a b) = render (a <* b)

seqBoth :: Applicative f => f a -> f b -> (f b, f a)
seqBoth x y = (x *> y, x <* y)

stDemo :: [Int]
stDemo = runST (newSTArray 2 0 >>= \arr ->
    (writeSTArray arr 0 1 *> writeSTArray arr 1 2) *> (readSTArray arr 0 <* writeSTArray arr 0 9) >>= \v ->
    stArrayToList arr >>= \xs -> pure (v : xs))

m1 :: M.Map Int String
m1 = M.fromList [(1, "a"), (2, "b"), (3, "c")]

m2 :: M.Map Int Bool
m2 = M.fromList [(2, True), (4, False)]

m3 :: M.Map Int Number
m3 = M.fromList [(3, 0.5), (5, 1.5)]

main :: IO ()
main = do
    print (Just 1 *> Just (2 :: Int), Just (1 :: Int) <* Just 2, [1, 2 :: Int] *> [10, 20 :: Int], Nothing *> Just (3 :: Int))
    print (fmap (+ 1) (Just (1 :: Int)) <* Just "x", (+) <$> Just (1 :: Int) <*> Just 2)
    print ([1, 2 :: Int] <* [True, False, True], (Nothing :: Maybe Int) <* Just True, [] *> [1 :: Int], [1 :: Int] *> ([] :: [Bool]))
    print (Nothing *> (undefined :: Maybe Int), (Nothing :: Maybe Int) <* (undefined :: Maybe Int), ([] :: [Int]) *> (undefined :: [Bool]), ([] :: [Int]) <* (undefined :: [Bool]))
    print (Just 1 *> Just 2 <* Just (3 :: Int), (+) <$> Just 1 <*> Just 2 *> Just (5 :: Int), Just 1 >> Just 2 *> Just (3 :: Int))
    print ((Right 1 :: Either String Int) *> Right 2, (Left "e" :: Either String Int) *> Right 2, (Right 1 :: Either String Int) <* Left "f", (Right 1 :: Either String Int) <* Right 2)
    print (zipWith (*>) [Just 1, Nothing] [Just 2, Just (3 :: Int)], zipWith (<*) [Just 1, Nothing] [Just 2, Just (3 :: Int)], foldr1 (*>) [Just 1, Just 2, Just (3 :: Int)])
    print (Id 1 *> Id (2 :: Int), Id (1 :: Int) <* Id 2, Two 1 2 *> Two 3 (4 :: Int), Two 1 2 <* Two 3 (4 :: Int))
    print (Tagged "a" 1 *> Tagged "b" (2 :: Int), Tagged "a" (1 :: Int) <* Tagged "b" 2, Tagged "a" (+ 1) <*> Tagged "b" (2 :: Int))
    print (seqBoth (Just 1) (Just (2 :: Int)), seqBoth [1, 2 :: Int] [3 :: Int], seqBoth (Id 1) (Id (2 :: Int)), seqBoth (Tagged "x" 1) (Tagged "y" (2 :: Int)))
    putStrLn (viaDict (SomeApp show (Just 1) (Just 2)))
    putStrLn (viaDict (SomeApp show [1, 2] [3]))
    putStrLn (viaDict (SomeApp show (Right 1 :: Either String Int) (Left "no")))
    putStrLn (viaThen (SomeApp show (Tagged "p" 1) (Tagged "q" 2)))
    putStrLn (viaBefore (SomeApp show (Two 1 2) (Two 3 4)))
    putStrLn "a" *> putStrLn "b"
    r <- (putStrLn "x" *> pure (1 :: Int)) <* putStrLn "y"
    print r
    s <- pure 10 <* (putStrLn "once" *> pure ()) <* putStrLn "twice"
    print (s :: Int)
    print stDemo
    print (M.toList (M.intersection m1 m2), M.toList (M.difference m1 m2))
    print (M.toList (M.intersection m1 m3), M.toList (M.difference m1 m3))
    print (M.toList (M.intersection m2 m1), M.toList (M.difference m3 m1), M.toList (M.intersection m2 m3))
    print (M.toList (M.intersection (M.fromList [(1 :: Int, "a"), (2, "b")]) (M.fromList [(2 :: Int, 0 :: Int)])))
    print (M.toList (M.difference (M.fromList [(1 :: Int, "a"), (2, "b")]) (M.fromList [(2 :: Int, True)])))
    print (S.toList (S.intersection (S.fromList [1, 2, 3 :: Int]) (S.fromList [2, 3, 4])), S.toList (S.difference (S.fromList [1, 2, 3 :: Int]) (S.fromList [2])))
