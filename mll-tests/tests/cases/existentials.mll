-- Existential types in data constructors

-- Basic existential: the type variable 'a' is hidden inside ShowBox
data ShowBox = forall a. MkShowBox a (a -> String)

showIt :: ShowBox -> String
showIt sb = case sb of
    MkShowBox x f -> f x

-- Multiple existential constructors
data AnyNum = forall a. MkInt a (a -> Integer)
            | forall a. MkStr a (a -> String)

extractStr :: AnyNum -> String
extractStr an = case an of
    MkInt x f -> show (f x)
    MkStr x f -> f x

-- Existential with multiple type variables
data Pair = forall a b. MkPair a b (a -> String) (b -> String)

showPair :: Pair -> String
showPair p = case p of
    MkPair x y fx fy -> fx x <> ", " <> fy y

-- Mix of existential and non-existential constructors
data Tagged = TagInt Integer
            | forall a. TagAny a (a -> String)

showTagged :: Tagged -> String
showTagged t = case t of
    TagInt n  -> show n
    TagAny x f -> f x

-- Existential wrapping a function (the hidden type is a function type)
data Reducer = forall a. MkReducer a (a -> Integer -> a) (a -> Integer)

runReducer :: Reducer -> [Integer] -> Integer
runReducer r xs = case r of
    MkReducer init step done -> done (foldl step init xs)

-- Nested existentials: existential inside a Maybe
data MaybeShow = forall a. JustShow a (a -> String) | NothingShow

showMaybe :: MaybeShow -> String
showMaybe ms = case ms of
    JustShow x f -> "Just " <> f x
    NothingShow  -> "Nothing"

-- Existential used as accumulator state
data Counter = forall s. MkCounter s (s -> s) (s -> Integer)

tick :: Counter -> Counter
tick c = case c of
    MkCounter s step get -> MkCounter (step s) step get

getCount :: Counter -> Integer
getCount c = case c of
    MkCounter s step get -> get s

-- Data type with both universal AND existential type variables
data Container a = forall b. MkContainer a b (b -> a)

extract :: Container a -> a
extract c = case c of
    MkContainer a b f -> a

convert :: Container a -> a
convert c = case c of
    MkContainer a b f -> f b

main :: IO ()
main = do
    -- Basic existential
    let box1 = MkShowBox 42 show
    let box2 = MkShowBox "hello" show
    putStrLn (showIt box1)
    putStrLn (showIt box2)

    -- Heterogeneous list of existentials
    let boxes = [MkShowBox 1 show, MkShowBox "two" show, MkShowBox 3 show]
    mapM_ (\b -> putStrLn (showIt b)) boxes

    -- Multiple constructors with existentials
    let n1 = MkInt 42 (\x -> x * 2)
    let n2 = MkStr True show
    putStrLn (extractStr n1)
    putStrLn (extractStr n2)

    -- Multiple existential variables
    let p = MkPair 42 "hello" show (\s -> s)
    putStrLn (showPair p)

    -- Mixed existential / non-existential constructors
    let t1 = TagInt 99
    let t2 = TagAny [1, 2, 3] show
    putStrLn (showTagged t1)
    putStrLn (showTagged t2)

    -- Existential wrapping a stateful reducer
    let sumReducer = MkReducer 0 (\acc x -> acc + x) (\x -> x)
    putStrLn (show (runReducer sumReducer [1, 2, 3, 4, 5]))

    -- Nested pattern match on existential
    let ms1 = JustShow 42 show
    let ms2 = NothingShow
    putStrLn (showMaybe ms1)
    putStrLn (showMaybe ms2)

    -- Counter: existential hides internal state type
    let counter = MkCounter 0 (\n -> n + 1) (\n -> n)
    let c3 = tick (tick (tick counter))
    putStrLn (show (getCount c3))

    -- Universal + existential type variables
    let c1 = MkContainer "direct" 42 show
    putStrLn (extract c1)
    putStrLn (convert c1)

    putStrLn "all existential tests passed"
