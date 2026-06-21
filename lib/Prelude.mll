-- MLL Prelude
-- This module is auto-imported into every MLL program.

-- FFI primitives
putStrLn :: String -> LuaIO "print" ()
putStr :: String -> LuaIO "io.write" ()
sqrt :: Number -> LuaPure "math.sqrt" Number

-- Process control
data ExitValue = Normal | Err Integer

-- Testing
assert :: Bool -> String -> IO ()
assert True _ = putStrLn "."
assert False msg = error msg

-- Common data types
data Any = AnyString String | AnyInteger Integer | AnyNumber Number | AnyBool Bool | AnyNull

data Either a b = Left a | Right b

data Ordering = LT | EQ | GT
    deriving Eq

-- Identity and combinators
id :: a -> a
id x = x

const :: a -> b -> a
const x _ = x

flip :: (a -> b -> c) -> b -> a -> c
flip f b a = f a b

-- Strict list operations (no lazy evaluation needed)
foldl :: (b -> a -> b) -> b -> [a] -> b
foldl _ acc [] = acc
foldl f acc (x:xs) = foldl f (f acc x) xs

foldr :: (a -> b -> b) -> b -> [a] -> b
foldr _ acc [] = acc
foldr f acc (x:xs) = f x (foldr f acc xs)

length :: [a] -> Integer
length [] = 0
length (_:xs) = 1 + length xs

reverse :: [a] -> [a]
reverse xs = go [] xs
    where
        go acc [] = acc
        go acc (x:rest) = go (x:acc) rest

-- List operations
concatMap :: (a -> [b]) -> [a] -> [b]
concatMap _ [] = []
concatMap f (x:xs) = prepend (f x) (concatMap f xs)
    where prepend [] rest = rest
          prepend (y:ys) rest = y : prepend ys rest

-- Longest prefix of elements satisfying the predicate. Lazy in the spine,
-- so it works on infinite lists (e.g. `takeWhile (< 5) [1..]`).
takeWhile :: (a -> Bool) -> [a] -> [a]
takeWhile _ [] = []
takeWhile p (x:xs) = if p x then x : takeWhile p xs else []

-- The remainder after takeWhile.
dropWhile :: (a -> Bool) -> [a] -> [a]
dropWhile _ [] = []
dropWhile p (x:xs) = if p x then dropWhile p xs else x : xs

-- True for the empty list.
null :: [a] -> Bool
null [] = True
null _  = False

-- Last element / everything but the last. Both error on the empty list.
last :: [a] -> a
last []       = error "last: empty list"
last [x]      = x
last (_ : xs) = last xs

init :: [a] -> [a]
init []       = error "init: empty list"
init [_]      = []
init (x : xs) = x : init xs

-- Flatten a list of lists.
concat :: [[a]] -> [a]
concat []         = []
concat (xs : xss) = xs ++ concat xss

-- n copies of x (empty when n <= 0).
replicate :: Integer -> a -> [a]
replicate n x = if n <= 0 then [] else x : replicate (n - 1) x

-- The infinite list [x, f x, f (f x), ...]. Lazy in the spine.
iterate :: (a -> a) -> a -> [a]
iterate f x = x : iterate f (f x)

-- Longest prefix satisfying p, paired with the remainder. Defined via
-- takeWhile/dropWhile to avoid `let (..) = .. in (fst.., snd..)`, which fails
-- the occurs-check at this polymorphic signature.
span :: (a -> Bool) -> [a] -> ([a], [a])
span p xs = (takeWhile p xs, dropWhile p xs)

-- Pair up two lists element-wise, stopping at the shorter. Lazy in the spine.
zip :: [a] -> [b] -> [(a, b)]
zip [] _ = []
zip _ [] = []
zip (x : xs) (y : ys) = (x, y) : zip xs ys

-- Inverse of zip.
unzip :: [(a, b)] -> ([a], [b])
unzip []         = ([], [])
unzip (p : rest) = case unzip rest of
    (as, bs) -> (fst p : as, snd p : bs)

-- Conjunction / disjunction of a Bool list. Short-circuiting.
and :: [Bool] -> Bool
and []       = True
and (x : xs) = if x then and xs else False

or :: [Bool] -> Bool
or []       = False
or (x : xs) = if x then True else or xs

-- Do any / all elements satisfy p? Short-circuiting and lazy:
-- `any (\x -> x > 3) [1 ..]` terminates.
any :: (a -> Bool) -> [a] -> Bool
any _ []       = False
any p (x : xs) = if p x then True else any p xs

all :: (a -> Bool) -> [a] -> Bool
all _ []       = True
all p (x : xs) = if p x then all p xs else False

-- Sum and product of an integer list.
sum :: [Integer] -> Integer
sum = foldl (\acc x -> acc + x) 0

product :: [Integer] -> Integer
product = foldl (\acc x -> acc * x) 1



-- Functor instances
fmap_IO :: (a -> b) -> IO a -> IO b
fmap_IO f action = action >>= \x -> pure (f x)

fmap_Maybe :: (a -> b) -> Maybe a -> Maybe b
fmap_Maybe _ Nothing = Nothing
fmap_Maybe f (Just x) = Just (f x)

fmap_Either :: (a -> b) -> Either c a -> Either c b
fmap_Either _ (Left x) = Left x
fmap_Either f (Right x) = Right (f x)

-- Applicative instances
ap_IO :: IO (a -> b) -> IO a -> IO b
ap_IO mf mx = mf >>= \f -> mx >>= \x -> pure (f x)

pure_Maybe :: a -> Maybe a
pure_Maybe x = Just x

ap_Maybe :: Maybe (a -> b) -> Maybe a -> Maybe b
ap_Maybe (Just f) (Just x) = Just (f x)
ap_Maybe _ _ = Nothing

pure_List :: a -> [a]
pure_List x = [x]

ap_List :: [a -> b] -> [a] -> [b]
ap_List [] _ = []
ap_List (f:fs) xs = concatMap_ap (map f xs) (ap_List fs xs)
    where concatMap_ap [] rest = rest
          concatMap_ap (y:ys) rest = y : concatMap_ap ys rest

pure_Either :: a -> Either c a
pure_Either x = Right x

ap_Either :: Either c (a -> b) -> Either c a -> Either c b
ap_Either (Right f) (Right x) = Right (f x)
ap_Either (Left e) _ = Left e
ap_Either _ (Left e) = Left e

-- Monad instances for Maybe
bind_Maybe :: Maybe a -> (a -> Maybe b) -> Maybe b
bind_Maybe Nothing _ = Nothing
bind_Maybe (Just x) f = f x

then_Maybe :: Maybe a -> Maybe b -> Maybe b
then_Maybe Nothing _ = Nothing
then_Maybe (Just _) b = b

-- Monad instance for [] (list bind and then)
bind_List :: [a] -> (a -> [b]) -> [b]
bind_List xs f = concatMap f xs

then_List :: [a] -> [b] -> [b]
then_List xs ys = concatMap (\_ -> ys) xs

-- Enum instance for Integer
succ_Integer :: Integer -> Integer
succ_Integer n = n + 1

pred_Integer :: Integer -> Integer
pred_Integer n = n - 1

toEnum_Integer :: Integer -> Integer
toEnum_Integer n = n

fromEnum_Integer :: Integer -> Integer
fromEnum_Integer n = n

enumFrom_Integer :: Integer -> [Integer]
enumFrom_Integer n = n : enumFrom_Integer (n + 1)

enumFromThen_Integer :: Integer -> Integer -> [Integer]
enumFromThen_Integer n m = enumFromThenHelper_Integer n (m - n)

enumFromThenHelper_Integer :: Integer -> Integer -> [Integer]
enumFromThenHelper_Integer x step = x : enumFromThenHelper_Integer (x + step) step

enumFromTo_Integer :: Integer -> Integer -> [Integer]
enumFromTo_Integer n m = if n > m then [] else n : enumFromTo_Integer (n + 1) m

enumFromThenTo_Integer :: Integer -> Integer -> Integer -> [Integer]
enumFromThenTo_Integer n next m = enumFromThenToHelper_Integer n (next - n) m

enumFromThenToHelper_Integer :: Integer -> Integer -> Integer -> [Integer]
enumFromThenToHelper_Integer x step m =
    if step > 0
    then if x > m then [] else x : enumFromThenToHelper_Integer (x + step) step m
    else if x < m then [] else x : enumFromThenToHelper_Integer (x + step) step m

-- Read instances
read_Integer :: String -> Integer
read_Integer s = ffi_tonumber s

read_Number :: String -> Number
read_Number s = ffi_tonumber_float s

read_Bool :: String -> Bool
read_Bool s = if s == "True" then True else False

read_String :: String -> String
read_String s = s

ffi_tonumber :: String -> LuaPure "tonumber" Integer
ffi_tonumber_float :: String -> LuaPure "tonumber" Number

infixl 4 <$>
infixl 4 <*>

-- Monadic combinators
mapM_ :: (a -> IO ()) -> [a] -> IO ()
mapM_ _ [] = pure ()
mapM_ f (x:xs) = f x >> mapM_ f xs

-- Conditional execution (non-strict evaluation makes this safe:
-- the action is thunked and only forced when the condition is true)
when :: Applicative f => Bool -> f () -> f ()
when cond action = if cond then action else pure ()

-- Convenience
print :: Show a => a -> IO ()
print x = putStrLn (show x)

-- Tuple accessors
fst :: (a, b) -> a
fst (x, _) = x

snd :: (a, b) -> b
snd (_, y) = y


elem :: a -> [a] -> Bool
elem a (x:xs)
    | a == x      = True
    | otherwise   = elem a xs
elem _ []     = False

-- head, tail, map, filter, take, drop, zipWith are implemented in the
-- Lua runtime to support lazy cons cells (infinite lists).
