-- Constrained existentials: `forall a. C a => Con a` guarantees the class
-- for the hidden type. Packing checks the instance at the only moment the
-- concrete type is known; unpacking makes exactly the declared classes
-- (and their superclasses) usable on the skolemized variable.

-- The classic heterogeneous-Show idiom, without a packed function
data Showable = forall a. Show a => Showable a

showIt :: Showable -> String
showIt s = case s of
    Showable x -> show x

-- Superclass entailment: Ord a guarantees Eq a on the hidden type
data OrdBox = forall a. Ord a => OrdBox a

selfEqual :: OrdBox -> Bool
selfEqual b = case b of
    OrdBox x -> x == x

-- Repacking an unpacked existential is fine (the hidden type never leaves)
repack :: Showable -> Showable
repack s = case s of
    Showable x -> Showable x

-- Function-clause patterns skolemize too, not just case branches
showDirect :: Showable -> String
showDirect (Showable x) = show x

-- Record syntax with an existential field: construction and matching work,
-- and a NON-existential field keeps its ordinary selector.
data Labeled = forall a. Show a => Labeled { hidden :: a, label :: String }

describe :: Labeled -> String
describe l = case l of
    Labeled x name -> name <> "=" <> show x

-- GADT syntax declares existentials implicitly: a signature variable that
-- does not reach the result type is hidden, and its context travels with it.
data GBox where
    MkGBox :: Show a => a -> GBox

showG :: GBox -> String
showG (MkGBox x) = show x

main :: IO ()
main = do
    -- Heterogeneous values built at different concrete types
    let xs = [Showable 42, Showable "hi", Showable True]
    mapM_ (\s -> putStrLn (showIt s)) xs

    -- Repacked values still carry their instance evidence
    mapM_ (\s -> putStrLn (showIt (repack s))) xs

    putStrLn (showDirect (Showable 3.5))

    putStrLn (show (selfEqual (OrdBox 7)))
    putStrLn (show (selfEqual (OrdBox "same")))

    let l = Labeled { hidden = 99, label = "answer" }
    putStrLn (describe l)
    putStrLn (label l)

    mapM_ (\b -> putStrLn (showG b)) [MkGBox 1, MkGBox "two", MkGBox False]

    putStrLn "all existential constraint tests passed"
