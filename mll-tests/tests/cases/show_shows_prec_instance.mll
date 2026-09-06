-- GHC's Show class: `show` and `showsPrec` default through each other, so
-- an instance may define either one. A hand-written showsPrec decides its
-- own parentheses (via showParen) and is called at precedence 11 from a
-- derived Show's positional fields and from `Just`; a show-only instance
-- is never parenthesized there (its showsPrec is the precedence-ignoring
-- default), exactly as under GHC.

-- showsPrec only, GHC-derived shape: parenthesize the application above
-- precedence 10, fields at 11.
data Cx = Cx Int Int

instance Show Cx where
    showsPrec d (Cx re im) =
        showParen (d > 10) (showString "Cx " . showsPrec 11 re . showString " " . showsPrec 11 im)

-- showsPrec only, infix-style at precedence 6: operands at 7.
data Sum = Sum Int Int

instance Show Sum where
    showsPrec d (Sum a b) = showParen (d > 6) (showsPrec 7 a . showString " :+: " . showsPrec 7 b)

-- show only: GHC never parenthesizes it, whatever it looks like.
data Raw = Raw Int

instance Show Raw where
    show (Raw n) = "Raw " <> show n

data Wrap = Wrap Cx Sum Raw (Maybe Cx) deriving Show
data Rec = Rec { rc :: Cx, rs :: Sum } deriving Show
data Tag = Tag | Tagged Int deriving Show

main :: IO ()
main = do
    print (Cx 1 (-2))
    print (Just (Cx 1 2))
    print (Sum 1 2, Sum (-1) 2)
    print (Just (Sum 1 2))
    print (Raw 3, Just (Raw 3))
    print (Wrap (Cx 0 1) (Sum 1 (-1)) (Raw 2) (Just (Cx 3 4)))
    print (Rec { rc = Cx 1 1, rs = Sum 2 2 })
    print [Cx 1 2, Cx 3 4]
    putStrLn (showsPrec 11 (Cx 1 2) "!")
    putStrLn (showsPrec 6 (Sum 1 2) "" <> "|" <> showsPrec 7 (Sum 1 2) "")
    putStrLn (shows (Raw 1) " and " <> showString "more" "")
    -- builtin and derived showsPrec: negatives above 6, applications at 11
    putStrLn (showsPrec 11 (-1 :: Int) "" <> showsPrec 7 (-1 :: Int) "" <> showsPrec 6 (-1 :: Int) "")
    putStrLn (showsPrec 11 (-1.5 :: Number) "" <> showsPrec 11 (-12345678901234567890 :: Integer) "")
    putStrLn (showsPrec 11 (Just (1 :: Int)) "" <> showsPrec 10 (Just (1 :: Int)) "" <> showsPrec 11 (Nothing :: Maybe Int) "")
    putStrLn (showsPrec 11 [1 :: Int, 2] "" <> showsPrec 11 (1 :: Int, "a") "" <> showsPrec 11 "s" "" <> showsPrec 11 True "")
    putStrLn (showsPrec 11 Tag "" <> showsPrec 11 (Tagged 1) "" <> showsPrec 10 (Tagged (-1)) "")
    putStrLn (showsPrec 11 (Just (Just (Tagged 2))) "" <> showsPrec 11 (Rec { rc = Cx 0 0, rs = Sum 0 0 }) "")
    print (Just (Just (-1 :: Int)), Just [Just (Cx 1 1)])
