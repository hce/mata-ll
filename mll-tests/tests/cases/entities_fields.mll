import Prelude hiding (head, error, either)
import EntFieldA
import qualified EntFieldB as B

data Q = Q { name :: Int, head :: Int }
    deriving (Show, Eq)

data Shape = Box { width :: Int, height :: Int } | Dot { width :: Int }
    deriving Show

grow :: Q -> Q
grow q = q { name = name q + 1 }

firstName :: Person -> String
firstName (Person n _) = n

sumRec :: B.Rec -> Int
sumRec r = r.other * 10 + B.either r

shadowed :: B.Rec -> Int
shadowed r = let other = 100 in r.other + other

mkR' :: Int -> Person
mkR' n = Person "z" n

main :: IO ()
main = do
    putStrLn (pname (mkP "x"))
    print (rename "y" (mkP "x"))
    print (name (Q { name = 7, head = 8 }), head (Q 1 2))
    print (grow (Q { head = 0, name = 1 }))
    print (Person { fullName = "Ada", tag = 3 })
    print (fullName (Person "Bo" 1), tag (Person "Cy" 2), firstName (Person "Di" 4))
    print ((Person "Ed" 5) { tag = 6 })
    print defaults
    print (error defaults, either defaults)
    print (defaults { either = 20 })
    print (B.total (B.Rec 3 4), B.total (B.bump (B.Rec 3 4)))
    print (B.either (B.Rec 5 6), B.other (B.Rec 5 6))
    putStrLn (B.showRec (B.Rec 1 2))
    print (width (Box 2 3), width (Dot 9), (Dot 1) { width = 5 })
    print (sumRec (B.Rec 3 4), shadowed (B.Rec 1 2), map (.other) [B.Rec 5 6, B.Rec 7 8])
    print ((mkR' 2).tag, (Q 3 4).head)
