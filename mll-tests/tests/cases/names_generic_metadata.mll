import Data.Generics

data A_B = C Int Int | D
    deriving (Generic)

data A = B_C Int | B_D
    deriving (Generic)

data S_1 = K { first :: String }
    deriving (Generic)

data S = I_1 { only :: Int } | I_0 Int
    deriving (Generic)

class GConMeta f where
    gconName :: f -> String
    gconArity :: f -> Int

instance (GConMeta a, GConMeta b) => GConMeta (a :+: b) where
    gconName (L1 x) = gconName x
    gconName (R1 y) = gconName y
    gconArity (L1 x) = gconArity x
    gconArity (R1 y) = gconArity y

instance Constructor c => GConMeta (C1 c f) where
    gconName c1 = conName c1
    gconArity c1 = conArity c1

instance GConMeta f => GConMeta (D1 d f) where
    gconName (D1 x) = gconName x
    gconArity (D1 x) = gconArity x

conNameOf :: (Generic a, GConMeta (Rep a)) => a -> String
conNameOf x = gconName (from x)

conArityOf :: (Generic a, GConMeta (Rep a)) => a -> Int
conArityOf x = gconArity (from x)

main :: IO ()
main = do
    k <- pure 1
    assert (conNameOf (C k 2) == "C" && conArityOf (C k 2) == 2) "constructor C of type A_B"
    assert (conNameOf (B_C k) == "B_C" && conArityOf (B_C k) == 1) "constructor B_C of type A"
    assert (conNameOf D == "D" && conNameOf B_D == "B_D") "constructors D of A_B and B_D of A"
    assert (datatypeName (from D) == "A_B" && datatypeName (from B_D) == "A") "the two datatype names"
    assert (conNameOf (K "x") == "K" && conNameOf (I_1 k) == "I_1" && conNameOf (I_0 k) == "I_0") "types S_1 and S"
    putStrLn "ok"
