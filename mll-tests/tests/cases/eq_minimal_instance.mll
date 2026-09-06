-- GHC's Eq class has two methods, `==` and `/=`, each defaulting through
-- the other: an instance may define either one alone. `/=` used to be a
-- free function the monomorphizer resolved through `==`, so an instance
-- defining only `/=` was rejected as "not a method of class 'Eq'".

data Color = Red | Green | Blue deriving Show

-- `/=` only: `==` is the class default `not (x /= y)`.
instance Eq Color where
    (/=) Red Red = False
    (/=) Green Green = False
    (/=) Blue Blue = False
    (/=) _ _ = True

data Mod = Mod Int deriving Show

-- `==` only (the usual shape): `/=` is the class default.
instance Eq Mod where
    (Mod a) == (Mod b) = a `mod` 3 == b `mod` 3

data Pair a = Pair a a deriving (Show, Eq)

allDiff :: Eq a => [a] -> Bool
allDiff [] = True
allDiff (x:xs) = all (\y -> x /= y) xs && allDiff xs

main :: IO ()
main = do
    print (Red == Red, Red /= Red, Red == Blue, Red /= Blue)
    print (Mod 1 == Mod 4, Mod 1 /= Mod 4, Mod 1 /= Mod 5)
    print (filter (/= Green) [Red, Green, Blue])
    print (allDiff [Red, Green, Blue], allDiff [Mod 1, Mod 4])
    -- derived and structural /=
    print (Pair 1 2 /= Pair 1 2, Pair 1 2 /= Pair 2 1)
    print ([1, 2] /= [1, 2], [1, 2] /= [1], (1, "a") /= (1, "b"), Just 1 /= Nothing)
    print (Just [Red] /= Just [Red], [Mod 1] /= [Mod 4], (Red, Mod 2) /= (Red, Mod 5))
    -- a hand-written instance threaded through the structural eq
    print ([Mod 1] == [Mod 4], Just (Mod 1) == Just (Mod 2), (Mod 1, Red) == (Mod 4, Red))
    print ((12345678901234567890 :: Integer) /= 12345678901234567890, (2 :: Integer) /= 3)
    print ((1 :: Int) /= 1, (1.5 :: Number) /= 2.5, "a" /= "a", True /= False, () /= ())
    print (zipWith (/=) [Red, Green] [Red, Blue], zipWith (/=) [Mod 1, Mod 2] [Mod 4, Mod 4])
    print (elem Green [Red, Blue], elem (Mod 7) [Mod 1, Mod 2])
