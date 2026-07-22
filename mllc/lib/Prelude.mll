-- MLL Prelude
-- This module is auto-imported into every MLL program.

-- FFI primitives
putStrLn :: String -> LuaIO "print" ()
putStr :: String -> LuaIO "io.write" ()
sqrt :: Number -> LuaPure "math.sqrt" Number

-- Console input, GHC-style. Reads one line from stdin WITHOUT the trailing
-- newline (io.read's default "l" format already strips it, matching GHC's
-- getLine). ffi_getLine is the raw internal binding: at end of input
-- io.read() returns nil, which LuaTry surfaces as Left rather than letting
-- a nil String escape. getLine turns that Left into a clean, catchable
-- error — mata-ll's analog of GHC's getLine throwing an isEOFError
-- exception (catch it with try/catch).
ffi_getLine :: LuaTry "io.read" (Either String String)

getLine :: IO String
getLine = do
    r <- ffi_getLine
    case r of
        Left _  -> error "Prelude.getLine: end of input"
        Right s -> pure s

-- Process control
data ExitValue = Normal | Err Integer

-- Testing
assert :: Bool -> String -> IO ()
assert True _ = putStrLn "."
assert False msg = error msg

-- Common data types
data Any = AnyString String | AnyInteger Integer | AnyNumber Number | AnyBool Bool | AnyNull

data Either a b = Left a | Right b
    deriving (Show)

data Ordering = LT | EQ | GT
    deriving (Show, Eq, Ord, Enum, Bounded)

-- Identity and combinators
id :: a -> a
id x = x

const :: a -> b -> a
const x _ = x

flip :: (a -> b -> c) -> b -> a -> c
flip f b a = f a b

-- Foldable instances for the builtin containers. `[]` in an instance head
-- is the bare, unapplied list constructor (kind Type -> Type), and
-- `Either c` is the partially applied Either — both must match the kind of
-- Foldable's class variable, which the compiler checks.
instance Foldable [] where
    foldr _ acc [] = acc
    foldr f acc (x:xs) = f x (foldr f acc xs)
    foldl _ acc [] = acc
    foldl f acc (x:xs) = foldl f (f acc x) xs

instance Foldable Maybe where
    foldr _ acc Nothing = acc
    foldr f acc (Just x) = f x acc
    foldl _ acc Nothing = acc
    foldl f acc (Just x) = f acc x

-- Folding an Either folds over Right and ignores Left, like GHC.
instance Foldable (Either c) where
    foldr _ acc (Left _) = acc
    foldr f acc (Right x) = f x acc
    foldl _ acc (Left _) = acc
    foldl f acc (Right x) = f acc x

-- Length-generic Foldable functions, defined over foldr/foldl.
-- (toList lives in Data.Foldable, matching GHC's Prelude exports.)
length :: Foldable t => t a -> Integer
length t = foldl (\n _ -> n + 1) 0 t

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

-- True for an empty structure. Lazy: only looks at the outermost
-- constructor, so it works on infinite lists.
null :: Foldable t => t a -> Bool
null t = foldr (\_ _ -> False) True t

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

-- Sum and product of any Foldable of numbers, generic over Num exactly as
-- GHC. The `0`/`1` seeds are polymorphic numeric literals (`fromInteger`).
sum :: (Foldable t, Num a) => t a -> a
sum t = foldl (\acc x -> acc + x) 0 t

product :: (Foldable t, Num a) => t a -> a
product t = foldl (\acc x -> acc * x) 1 t

-- Parity predicates over any Integral, defined via `rem` exactly as GHC:
-- `rem` truncates toward zero, so the sign of a negative argument never
-- reaches the comparison against 0 with a nonzero remainder of the wrong
-- sign — (-3) `rem` 2 is -1, and -1 == 0 is False, as required.
even :: Integral a => a -> Bool
even n = n `rem` 2 == 0

odd :: Integral a => a -> Bool
odd = not . even

-- Largest / smallest element. Both error on an empty structure.
maximum :: (Ord a, Foldable t) => t a -> a
maximum t = case foldr (\x xs -> x : xs) [] t of
    []     -> error "maximum: empty structure"
    (x:xs) -> foldl (\m y -> if y > m then y else m) x xs

minimum :: (Ord a, Foldable t) => t a -> a
minimum t = case foldr (\x xs -> x : xs) [] t of
    []     -> error "minimum: empty structure"
    (x:xs) -> foldl (\m y -> if y < m then y else m) x xs

-- Fold a list of monoid values into one (GHC's mconcat).
mconcat :: Monoid a => [a] -> a
mconcat xs = foldr mappend mempty xs

-- Map each element to a monoid and combine the results.
-- Monoid instances: String (concatenation, mempty "") and lists
-- (append, mempty []).
foldMap :: (Monoid m, Foldable t) => (a -> m) -> t a -> m
foldMap f t = foldr (\x acc -> mappend (f x) acc) mempty t

-- The Semigroup and Monoid classes. Ordinary source classes: the compiler
-- synthesizes each method's class constraint from these declarations (like
-- any user class), which is what makes an undetermined `mempty` an ambiguity
-- error at compile time. `mappend` is the named form of `(<>)`; both concat.
class Semigroup a where
    (<>) :: a -> a -> a

-- GHC's default: mappend is the named form of the superclass (<>). A user
-- instance that defines only mempty (the common, GHC-idiomatic shape) still
-- gets a working mappend — and with it foldMap/mconcat.
class Semigroup a => Monoid a where
    mempty  :: a
    mappend :: a -> a -> a
    mappend x y = x <> y

-- Semigroup and Monoid instances for the builtin containers.
--
-- String is opaque (Lua's string type), NOT `[Char]`, so it has no `++`:
-- its append is the runtime string-concatenation primitive `semigroup_String`
-- (Lua `..`), which the compiler exposes for exactly this purpose. Lists use
-- the ordinary `++` operator.
--
-- Note: `<>` on a concrete list is deliberately a compile error in mata-ll
-- (use `++`); the `Semigroup [a]` instance exists so polymorphic
-- Semigroup/Monoid code (e.g. foldMap) still resolves, and `mappend` gives
-- lists a working append.
instance Semigroup String where
    (<>) x y = semigroup_String x y

instance Semigroup [a] where
    (<>) xs ys = xs ++ ys

instance Monoid String where
    mempty = ""
    mappend x y = semigroup_String x y

instance Monoid [a] where
    mempty = []
    mappend xs ys = xs ++ ys



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

-- liftA2 instance implementations. liftA2 is an Applicative class
-- method (see the compiler's class registry for why it is a real
-- method and not <$>/<*> sugar: the IO runtime cannot carry a
-- function-valued action result).
liftA2_IO :: (a -> b -> c) -> IO a -> IO b -> IO c
liftA2_IO g ma mb = ma >>= \x -> mb >>= \y -> pure (g x y)

liftA2_Maybe :: (a -> b -> c) -> Maybe a -> Maybe b -> Maybe c
liftA2_Maybe g (Just x) (Just y) = Just (g x y)
liftA2_Maybe _ _ _ = Nothing

liftA2_List :: (a -> b -> c) -> [a] -> [b] -> [c]
liftA2_List g xs ys = concatMap (\x -> map (g x) ys) xs

liftA2_Either :: (a -> b -> c) -> Either e a -> Either e b -> Either e c
liftA2_Either g (Right x) (Right y) = Right (g x y)
liftA2_Either _ (Left e) _ = Left e
liftA2_Either _ _ (Left e) = Left e

pure_Either :: a -> Either c a
pure_Either x = Right x

ap_Either :: Either c (a -> b) -> Either c a -> Either c b
ap_Either (Right f) (Right x) = Right (f x)
ap_Either (Left e) _ = Left e
ap_Either _ (Left e) = Left e

-- Traversable instances for the builtin containers (heads at kind
-- Type -> Type, like Foldable's).
-- Built on liftA2 rather than <$>/<*> so the applicative never carries
-- a function-valued intermediate (which the IO runtime cannot represent).
instance Traversable [] where
    traverse _ [] = pure []
    traverse f (x:xs) = liftA2 (\y ys -> y : ys) (f x) (traverse f xs)

instance Traversable Maybe where
    traverse _ Nothing = pure Nothing
    traverse f (Just x) = fmap (\y -> Just y) (f x)

-- Traversing an Either visits Right and passes Left through, like GHC.
instance Traversable (Either c) where
    traverse _ (Left e) = pure (Left e)
    traverse f (Right x) = fmap (\y -> Right y) (f x)

-- Evaluate each action in the structure and collect the results.
sequenceA :: (Traversable t, Applicative f) => t (f a) -> f (t a)
sequenceA t = traverse (\x -> x) t

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

-- Fixities of the named (backtick) operators, matching the GHC Prelude.
-- div, mod, and seq are compiler builtins; their fixities live here so they
-- reach every module like the rest of the Prelude interface.
infixl 7 `div`, `mod`
infix  4 `elem`
infixr 0 `seq`

-- Monadic combinators
-- Result-discarding traversal (works in any monad).
mapM_ :: Monad m => (a -> m b) -> [a] -> m ()
mapM_ _ [] = pure ()
mapM_ f (x:xs) = f x >> mapM_ f xs

-- Result-collecting traversal (works in any monad).
mapM :: Monad m => (a -> m b) -> [a] -> m [b]
mapM _ [] = pure []
mapM f (x:xs) = f x >>= \y -> mapM f xs >>= \ys -> pure (y : ys)

sequence :: Monad m => [m a] -> m [a]
sequence [] = pure []
sequence (x:xs) = x >>= \y -> sequence xs >>= \ys -> pure (y : ys)

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


-- Is the element in the structure? Short-circuiting and lazy, so it
-- terminates on infinite lists when the element occurs.
elem :: (Eq a, Foldable t) => a -> t a -> Bool
elem a t = foldr (\x rest -> if a == x then True else rest) False t

-- head, tail, map, filter, take, drop, zipWith are implemented in the
-- Lua runtime to support lazy cons cells (infinite lists).
