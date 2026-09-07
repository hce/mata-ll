-- C6: type signatures on where-, let- and do-let-bound bindings. A
-- signature line (`h :: Bool -> String`, or `twice, thrice :: Int -> Int`)
-- may precede or follow the equations it names, and a multi-equation or
-- guarded local function carries one like any other. The binding is
-- checked against the declared type with its variables rigid, as a
-- top-level one is. A local signature's variables are the binding's own
-- (GHC without ScopedTypeVariables): `tag :: a -> String` under `label
-- :: Show a => a -> String` names a NEW `a`, so `tag` is polymorphic and
-- serves at Bool and String. An unconstrained polymorphic local
-- signature generalizes; a class-constrained one is rejected (HASKDIFF:
-- local bindings stay monomorphic in class-constrained variables).

wrap :: Int -> String
wrap n = h n <> "/" <> k
  where
    h :: Int -> String
    h y = show (y * 2)
    k :: String
    k = "k"

classify :: Int -> String
classify n = go n
  where
    go x
      | x < 0 = "neg"
      | otherwise = pos x
    go :: Int -> String
    pos :: Int -> String
    pos 0 = "zero"
    pos _ = "pos"

pairUp :: Int -> String -> ([Int], [String])
pairUp a b = (box a, box b)
  where
    box :: a -> [a]
    box y = [y]

label :: Show a => a -> String
label x = show x <> ":" <> tag True <> tag "s"
  where
    tag :: a -> String
    tag _ = "!"

square7 :: Int
square7 = let sq :: Int -> Int
              sq v = v * v
          in sq 7

-- polymorphic recursion keeps `nest` on the dictionary-passing generic
-- copy; the helper's own `a` must stay its own there too
nest :: Show a => Int -> a -> String
nest 0 x = show x <> mark x
  where
    mark :: a -> String
    mark _ = "."
nest n x = nest (n - 1) [x]

main :: IO ()
main = do
    putStrLn (wrap 21)
    putStrLn (classify (-1))
    putStrLn (classify 0)
    putStrLn (classify 5)
    print (pairUp 1 "one")
    putStrLn (label (1 :: Int))
    putStrLn (label "str")
    print square7
    putStrLn (nest 2 (1 :: Int))
    putStrLn (nest 1 "n")
    let inc :: Int -> Int
        inc v = v + 1
        twice, thrice :: Int -> Int
        twice v = inc (inc v)
        thrice v = inc (twice v)
    print (twice 1, thrice 1)
    let ident :: b -> b
        ident z = z
    print (ident (3 :: Int), ident "z")
    assert (twice 1 == 3) "let signature"
    putStrLn "ok"
