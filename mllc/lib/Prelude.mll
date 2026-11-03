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
data ExitValue = Normal | Err Int

-- Testing
assert :: Bool -> String -> IO ()
assert True _ = putStrLn "."
assert False msg = error msg

-- Common data types
data Any = AnyString String | AnyInt Int | AnyNumber Number | AnyBool Bool | AnyNull

data Either a b = Left a | Right b
    deriving (Show, Eq, Ord)

data Ordering = LT | EQ | GT
    deriving (Show, Eq, Ord, Enum, Bounded)

-- Identity and combinators
id :: a -> a
id x = x

const :: a -> b -> a
const x _ = x

flip :: (a -> b -> c) -> b -> a -> c
flip f b a = f a b

-- GHC's Prelude `subtract`: the section-friendly spelling of (-), so
-- `subtract 1` is the function `(- 1)` cannot be.
subtract :: Num a => a -> a -> a
subtract x y = y - x

-- The show combinators (GHC's Prelude): the building blocks of a
-- hand-written `showsPrec`. `shows` is showsPrec at precedence 0.
type ShowS = String -> String

shows :: Show a => a -> ShowS
shows x s = showsPrec 0 x s

showString :: String -> ShowS
showString str s = str <> s

showParen :: Bool -> ShowS -> ShowS
showParen b p s = if b then "(" <> p (")" <> s) else p s

-- Foldable instances for the builtin containers. `[]` in an instance head
-- is the bare, unapplied list constructor (kind Type -> Type), and
-- `Either c` is the partially applied Either — both must match the kind of
-- Foldable's class variable, which the compiler checks.
-- foldl' is the third Foldable method (GHC's Foldable has it too): the
-- STRICT left fold. The list instance is GHC.List's definition (base's
-- `foldr (\v fn -> oneShot (\z -> z `seq` fn (k z v))) id xs z0`, written
-- as the direct loop): at every cell the INCOMING accumulator is forced
-- before `f` is applied, and `f z x` is handed to the next step
-- unevaluated, where it is the incoming accumulator and gets forced in
-- turn. So the seed is forced whenever the structure is non-empty —
-- `foldl' (\_ x -> x) undefined [1]` raises as under GHC — and on `[]`
-- it is returned untouched (`length [foldl' (+) undefined []]` is 1).
-- Forcing the accumulator before the application rather than the
-- application's result (the previous shape, which never forced the seed)
-- is what GHC does, and it is the same amount of work: each `f z x` is
-- forced exactly once, by the step that receives it or by the caller of
-- the fold. The Maybe and Either instances are GHC's class default
-- (Data.Foldable: `f' x k = \z -> z `seq` k (f z x)`) at one element,
-- the same rule: the seed forced, the application handed on. Instances
-- that omit foldl' get that default from typechecker/prelude.rs.
instance Foldable [] where
    foldr _ acc [] = acc
    foldr f acc (x:xs) = f x (foldr f acc xs)
    foldl _ acc [] = acc
    foldl f acc (x:xs) = foldl f (f acc x) xs
    foldl' _ acc [] = acc
    foldl' f acc (x:xs) = acc `seq` foldl' f (f acc x) xs

instance Foldable Maybe where
    foldr _ acc Nothing = acc
    foldr f acc (Just x) = f x acc
    foldl _ acc Nothing = acc
    foldl f acc (Just x) = f acc x
    foldl' _ acc Nothing = acc
    foldl' f acc (Just x) = acc `seq` f acc x

-- Folding an Either folds over Right and ignores Left, like GHC.
instance Foldable (Either c) where
    foldr _ acc (Left _) = acc
    foldr f acc (Right x) = f x acc
    foldl _ acc (Left _) = acc
    foldl f acc (Right x) = f acc x
    foldl' _ acc (Left _) = acc
    foldl' f acc (Right x) = acc `seq` f acc x

-- Length-generic Foldable functions. length/sum/product/maximum/minimum
-- are STRICT left folds (the foldl' method), as in GHC (base's
-- `length`/`sum`/`product` are foldl', its list `maximum`/`minimum`
-- strict) — over the lazy `foldl` they built an O(n) thunk chain that
-- overflowed the Lua stack on large unfused lists (2.4e5 elements on Lua
-- 5.4, ~3e4 on LuaJIT).
-- (toList lives in Data.Foldable, matching GHC's Prelude exports.)
length :: Foldable t => t a -> Int
length t = foldl' (\n _ -> n + 1) 0 t

reverse :: [a] -> [a]
reverse xs = go [] xs
    where
        go acc [] = acc
        go acc (x:rest) = go (x:acc) rest

-- List operations
-- concatMap is concat of the mapped list view. (List comprehensions
-- desugar to it, so a filtering comprehension is a concat of mostly
-- empty fragments.)
concatMap :: Foldable t => (a -> [b]) -> t a -> [b]
concatMap f t = concat (map f (foldr (\x xs -> x : xs) [] t))

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

-- Flatten a list of lists, lazily (GHC's foldr (++) []). An empty
-- fragment is skipped by a self tail call: as `[] ++ rest` inside a foldr
-- step it would force the rest from within the step, one nested force per
-- empty fragment, and a long run of them would overflow the Lua stack
-- where GHC runs in constant stack. `ys ++ go xss` suspends the rest, so
-- the walk resumes only when the consumer reaches the end of `ys`.
concat :: Foldable t => t [a] -> [a]
concat t = go (foldr (\x xs -> x : xs) [] t)
  where
    go [] = []
    go (xs : xss) = case xs of
        [] -> go xss
        (y : ys) -> y : (ys ++ go xss)

-- n copies of x (empty when n <= 0).
replicate :: Int -> a -> [a]
replicate n x = if n <= 0 then [] else x : replicate (n - 1) x

-- Infinite lists, as in GHC. `repeat x` is one shared cyclic cell (the
-- tail refers to the list itself), so it costs no allocation per element
-- consumed; `cycle` ties the knot the same way.
repeat :: a -> [a]
repeat x = xs
  where xs = x : xs

cycle :: [a] -> [a]
cycle [] = error "Prelude.cycle: empty list"
cycle xs = ys
  where ys = xs ++ ys

-- The infinite list [x, f x, f (f x), ...]. Lazy in the spine.
iterate :: (a -> a) -> a -> [a]
iterate f x = x : iterate f (f x)

-- Longest prefix satisfying p, paired with the remainder. GHC's span is
-- `span _ xs@[] = (xs, xs); span p xs@(x:xs') | p x = let (ys, zs) = span
-- p xs' in (x : ys, zs) | otherwise = ([], xs)`: the list is matched and
-- `p x` decided BEFORE the pair exists (`span p ⊥` and `span (\_ -> ⊥)
-- [1]` are ⊥, where a pair of two traversals was defined), `p` runs once
-- per element, and the remainder IS the list's own suffix. Written
-- literally, the second component of a long satisfying prefix is a chain
-- of `zs` selectors one Lua frame deep per element (GHC enters a selector
-- thunk as a jump; `__force` recurses), so the two components are
-- instead two constant-stack walks that share the decisions through
-- `decided`, the list of `p x` suspended per cell by `map` and memoized
-- in the cell: `prefix` emits one cell per satisfying element and
-- `suffix` is a tail-recursive skip that returns the original suffix.
-- Both walk the list and its decisions side by side, so each component
-- demands the list's cells and `p` on them exactly where GHC's does and
-- an element's value is never demanded.
span :: (a -> Bool) -> [a] -> ([a], [a])
span p xs = case decided of
    [] -> ([], [])
    (b : _) -> if b then (prefix xs decided, suffix xs decided) else ([], xs)
  where
    decided = map p xs
    prefix (y : ys) (c : cs) = if c then y : prefix ys cs else []
    prefix _ _ = []
    suffix ys@(_ : ys') (c : cs) = if c then suffix ys' cs else ys
    suffix ys _ = ys

-- Pair up two lists element-wise, stopping at the shorter. Lazy in the spine.
-- Association-list lookup: the first pair whose key matches (GHC's
-- `lookup`, in the Prelude). Data.Map's `lookup` is the alias-qualified one.
lookup :: Eq a => a -> [(a, b)] -> Maybe b
lookup _ [] = Nothing
lookup k ((k2, v) : rest) = if k == k2 then Just v else lookup k rest

zip :: [a] -> [b] -> [(a, b)]
zip [] _ = []
zip _ [] = []
zip (x : xs) (y : ys) = (x, y) : zip xs ys

-- Inverse of zip. GHC's definition: a foldr whose step matches the pair
-- and builds both conses at once, taking the unzipped rest LAZILY (GHC's
-- `~(as, bs)`, here `fst r`/`snd r` in the suspended tails). Each tail is
-- one step of the fold, so walking either component runs in constant
-- stack and works on an infinite list; a strict match on the rest would
-- unzip the whole list, one nested call per element, before returning.
unzip :: [(a, b)] -> ([a], [b])
unzip xs = foldr (\(a, b) r -> (a : fst r, b : snd r)) ([], []) xs

-- Conjunction / disjunction, and whether any / all elements satisfy p.
-- Short-circuiting and lazy: `or (repeat True)` and `any (\x -> x > 3)
-- [1 ..]` terminate, and elements after the deciding one are never
-- forced. Foldable-generic through the structure's list view (its own
-- lazy foldr), walked by a self tail call: a foldr whose step returns the
-- suspended rest would force that rest from inside the step, one nested
-- force per element, and overflow the Lua stack on a long list where
-- GHC runs in constant stack.
and :: Foldable t => t Bool -> Bool
and t = go (foldr (\x xs -> x : xs) [] t)
  where
    go [] = True
    go (x : xs) = if x then go xs else False

or :: Foldable t => t Bool -> Bool
or t = go (foldr (\x xs -> x : xs) [] t)
  where
    go [] = False
    go (x : xs) = if x then True else go xs

any :: Foldable t => (a -> Bool) -> t a -> Bool
any p t = go (foldr (\x xs -> x : xs) [] t)
  where
    go [] = False
    go (x : xs) = if p x then True else go xs

all :: Foldable t => (a -> Bool) -> t a -> Bool
all p t = go (foldr (\x xs -> x : xs) [] t)
  where
    go [] = True
    go (x : xs) = if p x then go xs else False

-- Sum and product of any Foldable of numbers, generic over Num exactly as
-- GHC. The `0`/`1` seeds are polymorphic numeric literals (`fromInteger`).
sum :: (Foldable t, Num a) => t a -> a
sum t = foldl' (\acc x -> acc + x) 0 t

product :: (Foldable t, Num a) => t a -> a
product t = foldl' (\acc x -> acc * x) 1 t

-- Parity predicates over any Integral, defined via `rem` exactly as GHC:
-- `rem` truncates toward zero, so the sign of a negative argument never
-- reaches the comparison against 0 with a nonzero remainder of the wrong
-- sign — (-3) `rem` 2 is -1, and -1 == 0 is False, as required.
even :: Integral a => a -> Bool
even n = n `rem` 2 == 0

odd :: Integral a => a -> Bool
odd n = n `rem` 2 /= 0

-- General Integral-to-Num conversion, GHC's definition verbatim: lift to
-- Integer, lower with the target's fromInteger (so Integer -> Int wraps,
-- like GHC on a 64-bit machine).
fromIntegral :: (Integral a, Num b) => a -> b
fromIntegral n = fromInteger (toInteger n)

-- Rounding, with GHC's names and semantics but the result type fixed at
-- Int (the RealFrac generalization is deferred with the rest of the
-- numeric tower's upper rungs — see CAVEATS). The result is always an
-- Int, as GHC's `fromInteger . floor`: for a finite Number the exact
-- integer reduced modulo 2^64 into Int's range (`floor 1e19` is
-- -8446744073709551616, `floor 1e300` is 0), and 0 for NaN and the
-- infinities. The runtime primitives wrap Lua's math.floor/math.ceil,
-- which return a FLOAT when no integer fits; `floor` is the same
-- primitive LMath declares (an identical FFI re-declaration is not an
-- import collision), and LMath's Lua-named `ceil` is `ceiling`'s.
floor :: Number -> LuaPure "__mll_floor" Int
ceiling :: Number -> LuaPure "__mll_ceil" Int

-- Toward zero, as GHC.
truncate :: Number -> Int
truncate x = if x >= 0.0 then floor x else ceiling x

-- Round half to even, as GHC: a tie (fractional part exactly 0.5) goes
-- to the even neighbour. The fractional part is computed in doubles from
-- the floored double itself (`floor_Number`), never from the Int — the
-- Int is wrapped past 2^63 and `fromIntegral` of it would not give x
-- back. For |x| >= 2^52 a double is integral, so f is 0 and x rounds to
-- itself (0 for NaN and the infinities, whose f is NaN and fails both
-- comparisons). Below 2^52 the subtraction is exact for every x outside
-- (-0.5, 0) (Sterbenz: |x| and |floor x| are within a factor of two, or
-- the floor is 0); inside it the exact difference 1 + x lies above 0.5
-- and may round, but no lower than 0.5 itself — a tie then goes to the
-- even neighbour of n = -1, which is the 0 the exact fraction also picks.
round :: Number -> Int
round x =
    let n = floor x
        f = x - floor_Number x
    in if f < 0.5 then n
       else if f > 0.5 then n + 1
       else if even n then n else n + 1

-- The floored value as a Number: math.floor's own result, re-floated at
-- the FFI boundary (on Lua 5.3+ math.floor returns the integer subtype
-- when the result fits; a Number is a double on every host).
floor_Number :: Number -> LuaPure "math.floor" Number

-- Largest / smallest element: GHC's `foldl1' max` / `foldl1' min` over
-- the elements, the class's own `max`/`min` called as `max acc x` — not
-- a hand-written `>`/`<` test. The two differ exactly where the Ord
-- instance makes them differ: the defaults `max x y = if x <= y then y
-- else x` and `min x y = if x <= y then x else y` keep the LAST of equal
-- maxima and the FIRST of equal minima (`maximum [P 1 "a", P 1 "b"]` is
-- `P 1 "b"` when the instance compares the Int alone), an instance's own
-- `max`/`min` are what `maximum`/`minimum` use (lawless or not), and at
-- Number a NaN is never `<=` anything, so `maximum [1.0, nan]` is `1.0`,
-- `maximum [nan, 1.0]` is `nan`, and `maximum [0.0, -0.0]` is `-0.0`.
-- Both error on an empty structure.
maximum :: (Ord a, Foldable t) => t a -> a
maximum t = case foldr (\x xs -> x : xs) [] t of
    []     -> error "maximum: empty structure"
    (x:xs) -> foldl' max x xs

minimum :: (Ord a, Foldable t) => t a -> a
minimum t = case foldr (\x xs -> x : xs) [] t of
    []     -> error "minimum: empty structure"
    (x:xs) -> foldl' min x xs

-- GHC 9.14.1's Data.List.sort/sortBy (base 4.22, GHC.Internal.Data.OldList:
-- the natural mergesort with the four-way merge of GHC issue #24280),
-- clause for clause; mata-ll puts them in the Prelude like the other list
-- functions. Both are `sort_gt` under a "greater than" predicate: `sort`
-- under `(>)` and `sortBy cmp` under `cmp x y == GT` — base's spellings,
-- and they differ: at Number a NaN is never `>` anything but `compare` on
-- a NaN is GT, so `sort` and `sortBy compare` order a NaN-bearing list
-- differently under GHC, and here. The input is cut into monotone runs
-- (a strictly descending run is reversed as it is collected, an ascending
-- one kept), lists of up to four elements and the merge passes go through
-- the fused three- and four-way merges, and `merge` takes from the LEFT
-- run unless its head is greater, which keeps equal elements in input
-- order. Every predicate call is base's, with base's argument order, in
-- base's sequence, so a comparator that is no total order — NaN, a
-- comparator that is never EQ, `\_ _ -> GT` — yields GHC's permutation;
-- a stable sort of any other shape agrees with GHC only on total orders.
--
-- Two spellings differ from base without changing a single predicate call
-- or result: base collects an ascending run in a difference list
-- (`ascending b (\ys -> as (a:ys)) bs`) whose application nests one call
-- per element, a Lua call per element when the run is built, so an
-- already sorted input of 10^5 elements would overflow the host's stack;
-- here the run is a consed accumulator reversed at the end — the same
-- list. And base's bang patterns (`let !x = merge4 a b c d`) are `seq`.
sortBy :: (a -> a -> Ordering) -> [a] -> [a]
sortBy cmp = sort_gt (\x y -> cmp x y == GT)

sort :: Ord a => [a] -> [a]
sort = sort_gt (\x y -> x > y)

-- base's `actualSort`.
sort_gt :: (a -> a -> Bool) -> [a] -> [a]
sort_gt gt ns = case ns of
    []           -> []
    [a]          -> [a]
    [a, b]       -> merge [a] [b]
    [a, b, c]    -> merge3 [a] [b] [c]
    [a, b, c, d] -> merge4 [a] [b] [c] [d]
    _            -> merge_all (sequences ns)
  where
    sequences (a : b : xs)
        | gt a b    = descending b [a] xs
        | otherwise = ascending b [a] xs
    sequences xs = [xs]

    descending a acc (b : bs)
        | gt a b = descending b (a : acc) bs
    descending a acc bs = (a : acc) : sequences bs

    ascending a acc (b : bs)
        | not (gt a b) = ascending b (a : acc) bs
    ascending a acc bs = let x = reverse (a : acc) in seq x (x : sequences bs)

    merge_all [x] = x
    merge_all xs = merge_all (reduce_once xs)

    reduce_once [] = []
    reduce_once [a] = [a]
    reduce_once [a, b] = [merge a b]
    reduce_once [a, b, c] = [merge3 a b c]
    reduce_once [a, b, c, d, e] = [merge a b, merge3 c d e]
    reduce_once [a, b, c, d, e, f] = [merge3 a b c, merge3 d e f]
    reduce_once (a : b : c : d : xs) = let x = merge4 a b c d in seq x (x : reduce_once xs)

    merge l@(a : l') r@(b : r')
        | gt a b    = b : merge l r'
        | otherwise = a : merge l' r
    merge [] r = r
    merge l [] = l

    -- merge3 l r cs is merge (merge l r) cs, fused
    merge3 l@(a : l') r@(b : r') cs
        | gt a b    = merge3X b l r' cs
        | otherwise = merge3X a l' r cs
    merge3 [] r cs = merge r cs
    merge3 l [] cs = merge l cs

    merge3X x l r cs@(c : cs')
        | gt x c    = c : merge3X x l r cs'
        | otherwise = x : merge3 l r cs
    merge3X x l r [] = x : merge l r

    merge3Y l@(a : l') y r cs
        | gt a y    = y : merge3 l r cs
        | otherwise = a : merge3Y l' y r cs
    merge3Y [] x r cs = x : merge r cs

    -- merge4 l r cs ds is merge (merge l r) (merge cs ds), fused
    merge4 l@(a : l') r@(b : r') cs ds
        | gt a b    = merge4X b l r' cs ds
        | otherwise = merge4X a l' r cs ds
    merge4 [] r cs ds = merge3 r cs ds
    merge4 l [] cs ds = merge3 l cs ds

    merge4X x l r cs@(c : cs') ds@(d : ds')
        | gt c d    = merge4XY x l r d cs ds'
        | otherwise = merge4XY x l r c cs' ds
    merge4X x l r [] ds = merge3X x l r ds
    merge4X x l r cs [] = merge3X x l r cs

    merge4Y l@(a : l') r@(b : r') y cs ds
        | gt a b    = merge4XY b l r' y cs ds
        | otherwise = merge4XY a l' r y cs ds
    merge4Y l [] y cs ds = merge3Y l y cs ds
    merge4Y [] r y cs ds = merge3Y r y cs ds

    merge4XY x l r y cs ds
        | gt x y    = y : merge4X x l r cs ds
        | otherwise = x : merge4Y l r y cs ds

-- Map each element to a monoid and combine the results.
-- Monoid instances: String (concatenation, mempty "") and lists
-- (append, mempty []).
-- GHC's foldr (mappend . f) mempty, spelled as mconcat of the mapped list
-- view: the same right-nested combination for every lawful Monoid (mconcat
-- = foldr mappend mempty is a Monoid law, and the class default), but it
-- reaches the instance's own mconcat, so lists concatenate through concat
-- and Strings through one table.concat instead of a nested force per
-- element.
foldMap :: (Monoid m, Foldable t) => (a -> m) -> t a -> m
foldMap f t = mconcat (map f (foldr (\x xs -> x : xs) [] t))

-- The Semigroup and Monoid classes. Ordinary source classes: the compiler
-- synthesizes each method's class constraint from these declarations (like
-- any user class), which is what makes an undetermined `mempty` an ambiguity
-- error at compile time. `mappend` is the named form of `(<>)`; both concat.
infixr 6 <>
class Semigroup a where
    (<>) :: a -> a -> a

-- GHC's defaults: mappend is the named form of the superclass (<>), and
-- mempty and mconcat define each other (base: `mempty = mconcat []`,
-- `{-# MINIMAL mempty | mconcat #-}` — the pragma lives in the compiler,
-- beside the builtin classes' minimal definitions). A user instance that
-- defines only mempty (the common, GHC-idiomatic shape) still gets a
-- working mappend — and with it foldMap/mconcat — and one that defines
-- only mconcat gets mempty.
class Semigroup a => Monoid a where
    mempty  :: a
    mempty = mconcat []
    mappend :: a -> a -> a
    mappend x y = x <> y
    -- Fold a list of monoid values into one. A class method with GHC's
    -- default, exactly as in base — so an instance may override it with a
    -- shape that builds better than the right-nested fold (String does:
    -- see its instance below).
    mconcat :: [a] -> a
    mconcat xs = foldr mappend mempty xs

-- Semigroup and Monoid instances for the builtin containers.
--
-- String is opaque (Lua's string type), NOT `[Char]`, so it has no `++`:
-- its append is the runtime string-concatenation primitive `semigroup_String`
-- (Lua `..`), which the compiler exposes for exactly this purpose. Lists have
-- both `++` and, per GHC, `<>` — the `Semigroup [a]` instance below makes the
-- two synonyms, and also keeps polymorphic Semigroup/Monoid code (foldMap,
-- mconcat) resolving at list types.
instance Semigroup String where
    (<>) x y = semigroup_String x y

instance Semigroup [a] where
    (<>) xs ys = xs ++ ys

instance Monoid String where
    mempty = ""
    mappend x y = semigroup_String x y
    -- The default (foldr mappend mempty) copies the suffix at every step —
    -- O(total bytes^2) for a flat concatenation. The primitive collects the
    -- forced elements and joins once with table.concat; forcing is
    -- identical whenever the result is demanded (String is WHNF-atomic).
    mconcat xs = string_mconcat_prim xs

instance Monoid [a] where
    mempty = []
    mappend xs ys = xs ++ ys
    mconcat xss = concat xss

-- The MonadFail class, as in GHC's base: what a do-block does when a
-- failable bind pattern (`Just x <- action`) does not match. The compiler
-- desugars such a bind to a case whose fall-through arm calls `fail` with
-- GHC's message, so the block's monad decides what a mismatch means:
-- the list monad drops the element, Maybe yields Nothing, IO raises. A
-- monad without an instance (Either, ST, a user State monad) rejects
-- the bind at compile time, exactly as under GHC. An ordinary source
-- class like Semigroup: a user monad gets an instance by writing one.
class Monad m => MonadFail m where
    fail :: String -> m a

instance MonadFail [] where
    fail _ = []

instance MonadFail Maybe where
    fail _ = Nothing

-- IO raises GHC's IOException text, `user error (<message>)`, when the
-- action is RUN, not when it is built: `fail_IO` is the runtime primitive
-- that builds the raising action, so `try`/`catch` see it like any other
-- exception. The scoped `LuaIO s` monad has the same instance, registered
-- by the compiler beside its Monad instance (its constructor has no bare
-- spelling in an instance head).
instance MonadFail IO where
    fail msg = fail_IO msg



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
-- The list instance is the Applicative method `list_traverse` (see the
-- compiler's class registry): GHC's foldr over liftA2 by default, an
-- accumulating loop in the applicatives that override it below.
instance Traversable [] where
    traverse f xs = list_traverse f xs

-- The list traversals of IO (also LuaIO and ST), Maybe and Either. Each
-- runs the step for one element, then continues through the bind as a
-- tail call with the result consed onto an accumulator, and reverses at
-- the end: the effects (or the first failure) come in list order and the
-- rest of the list is not looked at after a failure, exactly as under
-- liftA2, and the result list is complete when the traversal returns, as
-- it is there. The results themselves stay unevaluated.
-- The list applicative keeps GHC's definition (the class default's): its
-- traversal is lazy in the rest, which a loop would not be.
list_traverse_List :: (a -> [b]) -> [a] -> [[b]]
list_traverse_List g xs = foldr (\x ys -> liftA2_List (\y zs -> y : zs) (g x) ys) [[]] xs

list_traverse_IO :: (a -> IO b) -> [a] -> IO [b]
list_traverse_IO g xs = go [] xs
  where
    go acc [] = pure (reverse acc)
    go acc (y : ys) = g y >>= \r -> go (r : acc) ys

list_traverse_Maybe :: (a -> Maybe b) -> [a] -> Maybe [b]
list_traverse_Maybe g xs = go [] xs
  where
    go acc [] = Just (reverse acc)
    go acc (y : ys) = case g y of
        Nothing -> Nothing
        Just r -> go (r : acc) ys

list_traverse_Either :: (a -> Either e b) -> [a] -> Either e [b]
list_traverse_Either g xs = go [] xs
  where
    go acc [] = Right (reverse acc)
    go acc (y : ys) = case g y of
        Left e -> Left e
        Right r -> go (r : acc) ys

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

-- Monad instance for Either e (GHC's: Left short-circuits, Right binds)
bind_Either :: Either e a -> (a -> Either e b) -> Either e b
bind_Either (Left e) _ = Left e
bind_Either (Right x) f = f x

then_Either :: Either e a -> Either e b -> Either e b
then_Either (Left e) _ = Left e
then_Either (Right _) b = b

-- Monad instance for [] (list bind and then)
bind_List :: [a] -> (a -> [b]) -> [b]
bind_List xs f = concatMap f xs

then_List :: [a] -> [b] -> [b]
then_List xs ys = concatMap (\_ -> ys) xs

-- Enum instance for Int
succ_Int :: Int -> Int
succ_Int n = n + 1

pred_Int :: Int -> Int
pred_Int n = n - 1

toEnum_Int :: Int -> Int
toEnum_Int n = n

fromEnum_Int :: Int -> Int
fromEnum_Int n = n

-- GHC.Enum's Int instance, transcribed (`eftInt`, `efdInt`, `efdtIntUp`,
-- `efdtIntDn`). The walks are bounded by construction: `enumFrom x` is
-- `enumFromTo x maxBound`, `enumFromThen x1 x2` runs to maxBound (upward)
-- or minBound (downward), and every walk tests its limit BEFORE the
-- addition that would pass it — `eftInt` stops on `x == y`, `efdtIntUp`
-- on `x > y - delta` — so no step ever computes `maxBound + 1`: on an
-- integer host that wraps to minBound and `n > m` then never holds, and
-- on a doubles-only host the addition is absorbed, both infinite loops
-- (N11). An Int range therefore terminates for every pair of bounds,
-- including the bounds themselves. The cons tails stay suspended, as in
-- base: `take 5 [1 ..]` walks five cells.
enumFrom_Int :: Int -> [Int]
enumFrom_Int n = enumFromTo_Int n maxBound

enumFromThen_Int :: Int -> Int -> [Int]
enumFromThen_Int x1 x2 = enumFromThenTo_Int x1 x2 (if x2 >= x1 then maxBound else minBound)

enumFromTo_Int :: Int -> Int -> [Int]
enumFromTo_Int n m = if n > m then [] else enumFromToUp_Int n m

-- `eftInt`'s `go`: `n <= m` holds at entry, so the equality is the exit and
-- the increment never leaves the range.
enumFromToUp_Int :: Int -> Int -> [Int]
enumFromToUp_Int x m = if x == m then [x] else x : enumFromToUp_Int (x + 1) m

-- GHC.Enum's direction rule (`enumFromThenTo x1 x2 y | x2 >= x1 = efdtIntUp
-- ..`): the walk is upward when the step is zero or positive and stops
-- once it passes the limit, so `[x, x .. y]` is infinitely many `x` when
-- `x <= y` and `[]` otherwise; only a negative step walks down.
enumFromThenTo_Int :: Int -> Int -> Int -> [Int]
enumFromThenTo_Int x1 x2 y =
    if x2 >= x1 then enumFromThenToUp_Int x1 x2 y else enumFromThenToDn_Int x1 x2 y

-- `efdtIntUp`: a limit below the second element leaves `[x1]` or `[]`;
-- otherwise `y >= x2 = x1 + delta`, so `y - delta` cannot wrap, and the
-- walk emits its last element when `x > y - delta` instead of computing
-- `x + delta` past the limit.
enumFromThenToUp_Int :: Int -> Int -> Int -> [Int]
enumFromThenToUp_Int x1 x2 y =
    if y < x2
    then if y < x1 then [] else [x1]
    else x1 : enumFromThenToUpGo_Int x2 (x2 - x1) (y - (x2 - x1))

enumFromThenToUpGo_Int :: Int -> Int -> Int -> [Int]
enumFromThenToUpGo_Int x delta lim =
    if x > lim then [x] else x : enumFromThenToUpGo_Int (x + delta) delta lim

-- `efdtIntDn`, the mirror: `y <= x2 = x1 + delta` with `delta < 0`, so
-- `y - delta` cannot wrap either.
enumFromThenToDn_Int :: Int -> Int -> Int -> [Int]
enumFromThenToDn_Int x1 x2 y =
    if y > x2
    then if y > x1 then [] else [x1]
    else x1 : enumFromThenToDnGo_Int x2 (x2 - x1) (y - (x2 - x1))

enumFromThenToDnGo_Int :: Int -> Int -> Int -> [Int]
enumFromThenToDnGo_Int x delta lim =
    if x < lim then [x] else x : enumFromThenToDnGo_Int (x + delta) delta lim

-- Enum instance for Bool: the semantics (and error texts) a derived
-- instance has — Bool is a builtin type, so it cannot carry a deriving
-- clause. The stepped ranges walk Int indices, like a derived instance.
succ_Bool :: Bool -> Bool
succ_Bool False = True
succ_Bool True = error "succ: already at maxBound for Bool"

pred_Bool :: Bool -> Bool
pred_Bool True = False
pred_Bool False = error "pred: already at minBound for Bool"

toEnum_Bool :: Int -> Bool
toEnum_Bool 0 = False
toEnum_Bool 1 = True
toEnum_Bool _ = error "toEnum: index out of range for Bool"

fromEnum_Bool :: Bool -> Int
fromEnum_Bool False = 0
fromEnum_Bool True = 1

enumFrom_Bool :: Bool -> [Bool]
enumFrom_Bool a = enumFromTo_Bool a True

enumFromTo_Bool :: Bool -> Bool -> [Bool]
enumFromTo_Bool False True = [False, True]
enumFromTo_Bool a b = if a == b then [a] else []

enumFromThen_Bool :: Bool -> Bool -> [Bool]
enumFromThen_Bool a b =
    enumFromThenTo_Bool a b (if fromEnum_Bool b >= fromEnum_Bool a then True else False)

enumFromThenTo_Bool :: Bool -> Bool -> Bool -> [Bool]
enumFromThenTo_Bool a b c =
    map toEnum_Bool (enumFromThenTo_Int (fromEnum_Bool a) (fromEnum_Bool b) (fromEnum_Bool c))

-- Enum instance for Integer (arbitrary precision). Mirrors Enum Int; the
-- arithmetic and comparisons dispatch to the Integer runtime helpers, and the
-- Int<->Integer conversions go through toInteger / fromInteger.
succ_Integer :: Integer -> Integer
succ_Integer n = n + 1

pred_Integer :: Integer -> Integer
pred_Integer n = n - 1

toEnum_Integer :: Int -> Integer
toEnum_Integer n = toInteger n

fromEnum_Integer :: Integer -> Int
fromEnum_Integer n = fromInteger n

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

-- The same direction rule as the Int helper (base's `enumDeltaToInteger x
-- delta lim | delta >= 0 = up_list ..`): a zero step walks upward.
enumFromThenToHelper_Integer :: Integer -> Integer -> Integer -> [Integer]
enumFromThenToHelper_Integer x step m =
    if step >= 0
    then if x > m then [] else x : enumFromThenToHelper_Integer (x + step) step m
    else if x < m then [] else x : enumFromThenToHelper_Integer (x + step) step m

-- Enum instance for (): GHC's `instance Enum ()` — the one value is index
-- 0, `succ`/`pred` fail, `[() ..]` and `[() .. ()]` are `[()]`, and the
-- stepped forms `[(), () ..]` / `[(), () .. ()]` are the infinite list of
-- units (base: `let many = ():many in many`). The error texts follow the
-- derived instances' (`succ_Bool` above), not base's "Prelude.Enum.().succ:
-- bad argument".
succ_Unit :: () -> ()
succ_Unit _ = error "succ: already at maxBound for ()"

pred_Unit :: () -> ()
pred_Unit _ = error "pred: already at minBound for ()"

toEnum_Unit :: Int -> ()
toEnum_Unit 0 = ()
toEnum_Unit _ = error "toEnum: index out of range for ()"

fromEnum_Unit :: () -> Int
fromEnum_Unit _ = 0

enumFrom_Unit :: () -> [()]
enumFrom_Unit _ = [()]

enumFromThen_Unit :: () -> () -> [()]
enumFromThen_Unit a b = a : enumFromThen_Unit a b

enumFromTo_Unit :: () -> () -> [()]
enumFromTo_Unit _ _ = [()]

enumFromThenTo_Unit :: () -> () -> () -> [()]
enumFromThenTo_Unit a b c = a : enumFromThenTo_Unit a b c

-- Bounded instance for (): GHC's `minBound = maxBound = ()`.
minBound_Unit :: ()
minBound_Unit = ()

maxBound_Unit :: ()
maxBound_Unit = ()

-- Enum instance for Number: GHC's `instance Enum Double` (base
-- GHC.Internal.Float / GHC.Internal.Real). `succ`/`pred` add and subtract
-- one, `toEnum` converts the Int, `fromEnum` truncates toward zero, and
-- the ranges are base's numericEnumFrom family, element for element:
--   numericEnumFrom n       = [n + k | k <- [0, 1 ..]]
--   numericEnumFromThen n m = [n + k * (m - n) | k <- [0, 1 ..]]
-- — the k-th element is computed from n and k, NOT by repeated addition
-- (the two differ in the last bits: `[0.1, 0.2 .. 1.0]` ends in 1.0 this
-- way and 0.9999999999999999 the other) — and the bounded forms stop at
-- the limit plus half a step: `enumFromTo n m` keeps elements `<= m + 1/2`
-- (`[1.5 .. 3]` is `[1.5, 2.5, 3.5]`), `enumFromThenTo e1 e2 e3` keeps
-- `<= e3 + (e2 - e1) / 2` when stepping up and `>= e3 + (e2 - e1) / 2`
-- when stepping down (a zero step repeats e1 forever, as in GHC).
succ_Number :: Number -> Number
succ_Number x = x + 1

pred_Number :: Number -> Number
pred_Number x = x - 1

toEnum_Number :: Int -> Number
toEnum_Number n = fromIntegral n

fromEnum_Number :: Number -> Int
fromEnum_Number x = truncate x

enumFrom_Number :: Number -> [Number]
enumFrom_Number n = numericEnumFromHelper_Number n 0

numericEnumFromHelper_Number :: Number -> Number -> [Number]
numericEnumFromHelper_Number n k = (n + k) : numericEnumFromHelper_Number n (k + 1)

enumFromThen_Number :: Number -> Number -> [Number]
enumFromThen_Number n m = numericEnumFromThenHelper_Number n (m - n) 0

numericEnumFromThenHelper_Number :: Number -> Number -> Number -> [Number]
numericEnumFromThenHelper_Number n step k =
    (n + k * step) : numericEnumFromThenHelper_Number n step (k + 1)

enumFromTo_Number :: Number -> Number -> [Number]
enumFromTo_Number n m = takeWhile (\x -> x <= m + 0.5) (enumFrom_Number n)

enumFromThenTo_Number :: Number -> Number -> Number -> [Number]
enumFromThenTo_Number e1 e2 e3 =
    takeWhile predicate (enumFromThen_Number e1 e2)
  where
    mid = (e2 - e1) / 2
    predicate x = if e2 >= e1 then x <= e3 + mid else x >= e3 + mid

-- Bounded instances for tuples: GHC's `instance (Bounded a, Bounded b) =>
-- Bounded (a, b)` and so on up to the 15-tuple, each bound the tuple of
-- its components' bounds.
instance (Bounded a, Bounded b) => Bounded (a, b) where
    minBound = (minBound, minBound)
    maxBound = (maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c) => Bounded (a, b, c) where
    minBound = (minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d) => Bounded (a, b, c, d) where
    minBound = (minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e) => Bounded (a, b, c, d, e) where
    minBound = (minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f) => Bounded (a, b, c, d, e, f) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g) => Bounded (a, b, c, d, e, f, g) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h) => Bounded (a, b, c, d, e, f, g, h) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h, Bounded i) => Bounded (a, b, c, d, e, f, g, h, i) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h, Bounded i, Bounded j) => Bounded (a, b, c, d, e, f, g, h, i, j) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h, Bounded i, Bounded j, Bounded k) => Bounded (a, b, c, d, e, f, g, h, i, j, k) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h, Bounded i, Bounded j, Bounded k, Bounded l) => Bounded (a, b, c, d, e, f, g, h, i, j, k, l) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h, Bounded i, Bounded j, Bounded k, Bounded l, Bounded m) => Bounded (a, b, c, d, e, f, g, h, i, j, k, l, m) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h, Bounded i, Bounded j, Bounded k, Bounded l, Bounded m, Bounded n) => Bounded (a, b, c, d, e, f, g, h, i, j, k, l, m, n) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

instance (Bounded a, Bounded b, Bounded c, Bounded d, Bounded e, Bounded f, Bounded g, Bounded h, Bounded i, Bounded j, Bounded k, Bounded l, Bounded m, Bounded n, Bounded o) => Bounded (a, b, c, d, e, f, g, h, i, j, k, l, m, n, o) where
    minBound = (minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound, minBound)
    maxBound = (maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound, maxBound)

-- Read instances: one runtime lexer with GHC's Text.Read.Lex grammar
-- (surrounding space, any depth of parentheses, `-` before a number,
-- radix prefixes, a Haskell string literal at String); anything else
-- raises GHC's "Prelude.read: no parse".
read_Int :: String -> LuaPure "__mll_read_int" Int

read_Number :: String -> LuaPure "__mll_read_number" Number

read_Bool :: String -> LuaPure "__mll_read_bool" Bool

read_String :: String -> LuaPure "__mll_read_string" String

ffi_tonumber :: String -> LuaPure "tonumber" Int
ffi_tonumber_float :: String -> LuaPure "tonumber" Number

infixr 8 ^

-- Exponentiation by squaring: `x ^ n` = x multiplied by itself n times, using
-- the Num `*` (so it is exact at Integer). The exponent is an Int; a negative
-- exponent is an error, as in GHC. `powAcc b k acc = acc * b^k`.
(^) :: Num a => a -> Int -> a
x ^ n =
    if n < 0 then error "Negative exponent"
    else powAcc x n 1

powAcc :: Num a => a -> Int -> a -> a
powAcc b k acc =
    if k == 0 then acc
    else powAcc (b * b) (k `div` 2) (if k `mod` 2 == 1 then acc * b else acc)

-- Fixities of the operators the compiler provides and of the named
-- (backtick) operators, matching the GHC Prelude. The symbolic operators
-- defined in this file declare theirs next to their definitions.
infixr 9 .
infixl 9 !!
infixl 7 *, /, `div`, `mod`, `quot`, `rem`
infixl 6 +, -
infixr 5 :, ++
infix  4 ==, /=, <, <=, >=, >, `elem`, `notElem`
infixl 4 <$>, <*>
infixr 3 &&
infixr 2 ||
infixl 1 >>, >>=
infixr 0 $, `seq`

-- Monadic combinators
-- Result-discarding traversal (works in any monad), over the structure's
-- list view. Each step continues through `>>=` into a self call rather
-- than `f x >> rest` with a suspended rest: a `>>` that returns its second
-- argument (Maybe's, Either's, an Identity's) would force that rest from
-- inside the step, one nested force per element, and overflow the Lua
-- stack on a long list. Through the bind the continuation is a tail call
-- in every monad whose bind tail-calls it (IO's runner, Maybe, Either).
-- `m >>= \_ -> k` is `m >> k` by the Monad laws, and the default `>>`.
mapM_ :: (Foldable t, Monad m) => (a -> m b) -> t a -> m ()
mapM_ f t = go (foldr (\x xs -> x : xs) [] t)
  where
    go ys = case ys of
        [] -> pure ()
        (x : xs) -> f x >>= \_ -> go xs

-- Result-collecting traversal (works in any monad).
-- mapM = traverse and sequence = sequenceA at Monad, exactly GHC's
-- definitions (Applicative is Monad's superclass, so the constraint
-- entails what traverse needs).
mapM :: (Traversable t, Monad m) => (a -> m b) -> t a -> m (t b)
mapM f t = traverse f t

sequence :: (Traversable t, Monad m) => t (m a) -> m (t a)
sequence t = sequenceA t

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
-- terminates on infinite lists when the element occurs; walked like `any`.
elem :: (Eq a, Foldable t) => a -> t a -> Bool
elem a t = go (foldr (\x xs -> x : xs) [] t)
  where
    go [] = False
    go (x : xs) = if a == x then True else go xs


-- Complement of `elem`, Foldable-generic like it.
notElem :: (Eq a, Foldable t) => a -> t a -> Bool
notElem a t = not (elem a t)

-- ---------------------------------------------------------------------------
-- The rest of GHC's Prelude that is expressible here, with GHC's definitions.
-- The PSE dogfood (2026-09-12) wrote `either`/`maybe` and found them absent;
-- the program corpus had been dodging the gap with local `either'` copies.
-- ---------------------------------------------------------------------------

-- Case analysis for Either and Maybe.
either :: (a -> c) -> (b -> c) -> Either a b -> c
either f _ (Left x) = f x
either _ g (Right y) = g y

maybe :: b -> (a -> b) -> Maybe a -> b
maybe def _ Nothing = def
maybe _ f (Just x) = f x

-- Tuple/function shape conversions.
curry :: ((a, b) -> c) -> a -> b -> c
curry f x y = f (x, y)

uncurry :: (a -> b -> c) -> (a, b) -> c
uncurry f p = f (fst p) (snd p)

-- `asTypeOf` is `const` with a signature that unifies its arguments' types.
asTypeOf :: a -> a -> a
asTypeOf x _ = x

-- Iterate f until the predicate holds, starting with x itself.
until :: (a -> Bool) -> (a -> a) -> a -> a
until p f x = if p x then x else until p f (f x)

-- Strict application: the argument is forced before the call.
infixr 0 $!
($!) :: (a -> b) -> a -> b
f $! x = x `seq` f x

-- Bind with the arguments flipped.
infixr 1 =<<
(=<<) :: Monad m => (a -> m b) -> m a -> m b
f =<< m = m >>= f

-- Replace every element of a Functor with one value (GHC's `<$`, a Functor
-- method there; a plain function over fmap here, like `<$>` in spirit).
infixl 4 <$
(<$) :: Functor f => a -> f b -> f a
x <$ m = fmap (const x) m

-- Evaluate each action, discarding the results (Foldable-generic like mapM_).
sequence_ :: (Foldable t, Monad m) => t (m a) -> m ()
sequence_ t = mapM_ (\x -> x) t

-- Greatest common divisor and least common multiple over any Integral,
-- GHC's definitions: `gcd` is never negative, `gcd 0 0` is 0, and `lcm`
-- of anything with 0 is 0.
gcd :: Integral a => a -> a -> a
gcd x y = go (abs x) (abs y)
  where
    go a 0 = a
    go a b = go b (a `rem` b)

lcm :: Integral a => a -> a -> a
lcm _ 0 = 0
lcm 0 _ = 0
lcm x y = abs ((x `quot` gcd x y) * y)

-- Prefix/suffix split at an index. GHC's definition, not the Report's
-- `(take n xs, drop n xs)`: strict in n, and for n > 0 in the list to
-- WHNF, while the pair's parts stay lazy (`head (fst (splitAt 3 (1 : ⊥)))`
-- is 1).
splitAt :: Int -> [a] -> ([a], [a])
splitAt n ls
  | n <= 0 = ([], ls)
  | otherwise = splitAt' n ls
  where
    splitAt' _ [] = ([], [])
    splitAt' 1 (x : xs) = ([x], xs)
    splitAt' m (x : xs) = (x : fst r, snd r)
      where
        r = splitAt' (m - 1) xs

-- Longest prefix NOT satisfying p, paired with the remainder (the dual of span).
break :: (a -> Bool) -> [a] -> ([a], [a])
break p xs = span (\x -> not (p x)) xs

-- Running folds. `scanl` and `scanr` keep the seed as the first/last
-- element; the `1` variants start from the list's own first/last element
-- and are empty on an empty list.
-- scanl is GHC's definition clause for clause: the seed is emitted FIRST
-- and the list is matched inside the suspended tail, so `scanl f q ⊥` is
-- `q : ⊥`, `take 2 (scanl (+) 0 (1 : ⊥))` is `[0, 1]`, and the knot `ps =
-- scanl (+) 1 ps` ties (the cell exists before `ps` is inspected). A
-- clause per list shape would match the list before producing the seed.
scanl :: (b -> a -> b) -> b -> [a] -> [b]
scanl f q ls = q : (case ls of
    [] -> []
    (x:xs) -> scanl f (f q x) xs)

scanl1 :: (a -> a -> a) -> [a] -> [a]
scanl1 _ [] = []
scanl1 f (x:xs) = scanl f x xs

-- GHC's definitions: the scanned rest `qs` is bound lazily (GHC's
-- `qs@(q:_)` in a where), so the result's spine is one step of the scan
-- per cell, constant stack to walk and productive on an infinite list.
-- Only an element's VALUE demands the next one. (The head of the rest is
-- taken with a local match rather than `head`, so `head` stays a name no
-- Prelude code depends on: a user program may redefine it at a narrower
-- type. The rest is never empty; its `[]` arm is unreachable.)
scanr :: (a -> b -> b) -> b -> [a] -> [b]
scanr _ acc [] = [acc]
scanr f acc (x:xs) = next qs : qs
  where
    qs = scanr f acc xs
    next (q : _) = f x q
    next [] = error "Prelude.scanr: empty rest"

scanr1 :: (a -> a -> a) -> [a] -> [a]
scanr1 _ [] = []
scanr1 _ [x] = [x]
scanr1 f (x:xs) = next qs : qs
  where
    qs = scanr1 f xs
    next (q : _) = f x q
    next [] = error "Prelude.scanr1: empty rest"

-- Folds without a seed: the structure must be non-empty. Foldable-generic
-- through the same list view `maximum`/`minimum` use.
foldr1 :: Foldable t => (a -> a -> a) -> t a -> a
foldr1 f t = case foldr (\x xs -> x : xs) [] t of
    [] -> error "Prelude.foldr1: empty list"
    xs -> go xs
  where
    go [x] = x
    go (x : xs) = f x (go xs)

foldl1 :: Foldable t => (a -> a -> a) -> t a -> a
foldl1 f t = case foldr (\x xs -> x : xs) [] t of
    [] -> error "Prelude.foldl1: empty list"
    (x : xs) -> foldl f x xs

-- Three-way zips, stopping at the shortest list.
zip3 :: [a] -> [b] -> [c] -> [(a, b, c)]
zip3 (x : xs) (y : ys) (z : zs) = (x, y, z) : zip3 xs ys zs
zip3 _ _ _ = []

zipWith3 :: (a -> b -> c -> d) -> [a] -> [b] -> [c] -> [d]
zipWith3 f (x : xs) (y : ys) (z : zs) = f x y z : zipWith3 f xs ys zs
zipWith3 _ _ _ _ = []

-- GHC's definition, lazy in the unzipped rest like `unzip`.
unzip3 :: [(a, b, c)] -> ([a], [b], [c])
unzip3 xs = foldr (\(a, b, c) r -> (a : first r, b : second r, c : third r)) ([], [], []) xs
  where
    first (as, _, _) = as
    second (_, bs, _) = bs
    third (_, _, cs) = cs

-- head, tail, map, filter, take, drop, zipWith are implemented in the
-- Lua runtime to support lazy cons cells (infinite lists).
