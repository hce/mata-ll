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
    MkPair x y fx fy -> fx x ++ ", " ++ fy y

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

    -- Existential with constraint (Show a =>)
    -- The constraint is parsed but currently MLL uses monomorphization,
    -- so the Show dictionary is resolved at construction time
    putStrLn "all existential tests passed"
