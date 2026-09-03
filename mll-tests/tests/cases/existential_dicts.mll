-- Constrained existentials carry their class dictionaries (GHC's
-- representation): `Showable x` packs the Show dictionary for x's type as
-- a hidden trailing field, a match binds it back, and every method use at
-- the hidden type — named (`show x`), infix (`a < b`), through a container
-- (`show [x]`, `show (Just x)`, `show (x, x)`), through a constrained
-- helper (`tagged "v" x`), via a superclass (`==` from an `Ord a` given)
-- — dispatches through it. Before, the solver accepted the constraint with
-- no runtime evidence and `show x` fell to the type-erased runtime show:
-- `Circle 3` printed as `(1,3)`, `[] :: [Int]` as `Nothing`.
-- Also: the unsaturated constructor (`map Showable`), re-packing, a
-- `case` with guards, a record-syntax existential, laziness of the
-- payload, and a pack inside a DICTIONARY-PASSING body (mkBox at 17 types)
-- where the dictionary is the body's own parameter.

data Shape = Circle Int | Square Int Int deriving (Show, Eq, Ord)
data Showable = forall a. Show a => Showable a
data Orderable = forall a. (Show a, Ord a) => Orderable a a
data Pair = forall a b. (Show a, Show b) => Pair a b
newtype Wrap = Wrap Int deriving (Show, Eq, Ord)
data K1 = K1 deriving Show
data K2 = K2 deriving Show
data K3 = K3 deriving Show
data K4 = K4 deriving Show
data K5 = K5 deriving Show
data K6 = K6 deriving Show
data K7 = K7 deriving Show
data K8 = K8 deriving Show
data K9 = K9 deriving Show
data K10 = K10 deriving Show
data K11 = K11 deriving Show
data K12 = K12 deriving Show
data K13 = K13 deriving Show
data K14 = K14 deriving Show
data K15 = K15 deriving Show
data K16 = K16 deriving Show
data K17 = K17 deriving Show

mkBox :: Show a => a -> Showable
mkBox x = Showable [x, x]
data Rec = forall a. Show a => Rec { payload :: a, label :: String }

describe :: Showable -> String
describe (Showable x) = show x

describeAll :: Showable -> String
describeAll (Showable x) = show [x] <> " " <> show (Just x) <> " " <> show (x, x) <> " " <> bracket x

bracket :: Show b => b -> String
bracket v = "<" <> show v <> ">"

tagged :: Show a => String -> a -> String
tagged t v = t <> "=" <> show v

viaHelper :: Showable -> String
viaHelper (Showable x) = tagged "v" x <> "/" <> tagged "l" [x]

cmpBoth :: Orderable -> String
cmpBoth (Orderable a b) = show (compare a b, a == b, max a b, a < b)

showPair :: Pair -> String
showPair (Pair a b) = show a <> "&" <> show b

repack :: Showable -> Showable
repack (Showable x) = Showable (Just x)

byCase :: Showable -> String
byCase s = case s of
    Showable x | show x == "Circle 3" -> "three"
    Showable x -> "other:" <> show x

byLet :: Showable -> String
byLet s = let r = case s of Showable x -> show x in r <> "!"

recShow :: Rec -> String
recShow (Rec p l) = l <> ":" <> show p

main :: IO ()
main = do
    putStrLn (describe (Showable (Circle 3)))
    putStrLn (describeAll (Showable (Square 1 2)))
    putStrLn (describeAll (Showable (Wrap 7)))
    putStrLn (viaHelper (Showable (Circle 9)))
    putStrLn (cmpBoth (Orderable (Circle 1) (Square 1 2)))
    putStrLn (cmpBoth (Orderable (Wrap 5) (Wrap 5)))
    putStrLn (cmpBoth (Orderable "b" "a"))
    putStrLn (showPair (Pair (Circle 2) "s"))
    putStrLn (describe (repack (Showable (Circle 4))))
    putStrLn (describe (repack (repack (Showable (3 :: Int)))))
    putStrLn (byCase (Showable (Circle 3)))
    putStrLn (byCase (Showable (Wrap 3)))
    putStrLn (byLet (Showable [Just (Wrap 1)]))
    putStrLn (recShow (Rec (Circle 8) "circle"))
    -- Unsaturated constructor.
    mapM_ (putStrLn . describe) (map Showable [Circle 5, Circle 6])
    putStrLn (describe (Showable (map Showable [Wrap 1] `seq` Wrap 2)))
    -- Laziness: packing never forces the payload.
    let lazyBox = Showable (error "never forced" :: Int)
    case lazyBox of
        Showable _ -> putStrLn "unpacked without forcing"
    print (length [Showable (Circle 1), lazyBox])
    -- 17 instantiations: mkBox is compiled once with dictionary passing,
    -- and its pack captures the parameter dictionary.
    putStrLn (describe (mkBox K1))
    putStrLn (describe (mkBox K2))
    putStrLn (describe (mkBox K3))
    putStrLn (describe (mkBox K4))
    putStrLn (describe (mkBox K5))
    putStrLn (describe (mkBox K6))
    putStrLn (describe (mkBox K7))
    putStrLn (describe (mkBox K8))
    putStrLn (describe (mkBox K9))
    putStrLn (describe (mkBox K10))
    putStrLn (describe (mkBox K11))
    putStrLn (describe (mkBox K12))
    putStrLn (describe (mkBox K13))
    putStrLn (describe (mkBox K14))
    putStrLn (describe (mkBox K15))
    putStrLn (describe (mkBox K16))
    putStrLn (describe (mkBox K17))
    putStrLn (describe (mkBox (Circle 17)))
