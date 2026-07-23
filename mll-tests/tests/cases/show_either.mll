-- Regression: Show for Either (was: "No instance for 'Show (Either …)'").
-- The Prelude's `data Either a b` derives Show like any ordinary tagged ADT,
-- so nesting/parenthesization matches derived Show everywhere else
-- (GHC showsPrec 11 on constructor arguments).

data Suit = Clubs | Spades
    deriving (Show)

main :: IO ()
main = do
    -- Plain payloads on both sides
    assert (show (Left 5 :: Either Int String) == "Left 5") "show Left Int"
    assert (show (Right 5 :: Either String Int) == "Right 5") "show Right Int"
    -- String payloads are unquoted (documented mata-ll deviation from GHC)
    assert (show (Left "x" :: Either String Int) == "Left \"x\"") "show Left String"
    assert (show (Right "hi" :: Either Int String) == "Right \"hi\"") "show Right String"
    -- Constructor arguments are parenthesized like any derived Show
    assert (show (Right (Just 5) :: Either String (Maybe Int)) == "Right (Just 5)")
        "show Right (Just …)"
    assert (show (Left (Left 3) :: Either (Either Int String) String) == "Left (Left 3)")
        "show nested Either"
    -- Lists and tuples in the payload use their structural show
    assert (show (Right [1, 2, 3] :: Either String [Int]) == "Right [1,2,3]")
        "show Right list"
    assert (show (Left [1, 2] :: Either [Int] Int) == "Left [1,2]") "show Left list"
    assert (show (Right (1, "a") :: Either String (Int, String)) == "Right (1,\"a\")")
        "show Right tuple"
    -- A user ADT payload dispatches to its own derived Show instance
    assert (show (Right Spades :: Either String Suit) == "Right Spades") "show Right user ADT"
    -- Either inside other containers
    assert (show [Left 1, Right 2 :: Either Int Int] == "[Left 1,Right 2]")
        "show list of Either"
    assert (show (Just (Left 7) :: Maybe (Either Int String)) == "Just (Left 7)")
        "show Maybe of Either"
    -- Negative payloads are parenthesized (showsPrec 11)
    assert (show (Left (-3) :: Either Int String) == "Left (-3)") "show Left negative"
    putStrLn "show_either: all assertions passed"
