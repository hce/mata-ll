-- ghc_regr009: Multiple typeclass constraints - each function uses one constraint,
-- multiple instances satisfy all of them

class Named a where
    name :: a -> String

class Scored a where
    score :: a -> Integer

class Ranked a where
    rank :: a -> String

data Player = Player String Integer
    deriving (Show, Eq)

data Team = Team String Integer
    deriving (Show, Eq)

instance Named Player where
    name (Player n _) = n

instance Scored Player where
    score (Player _ s) = s

instance Ranked Player where
    rank x = name x ++ ":" ++ show (score x)

instance Named Team where
    name (Team n _) = n

instance Scored Team where
    score (Team _ s) = s

instance Ranked Team where
    rank x = name x ++ ":" ++ show (score x)

-- Each function uses exactly one typeclass constraint
getName :: Named a => a -> String
getName x = name x

getScore :: Scored a => a -> Integer
getScore x = score x

getRank :: Ranked a => a -> String
getRank x = rank x

-- Higher score wins (uses only Scored)
higherScore :: Scored a => a -> a -> Bool
higherScore a b = score a > score b

-- Find by name (uses only Named)
findByName :: Named a => String -> [a] -> Bool
findByName _ []     = False
findByName n (x:xs)
    | name x == n   = True
    | otherwise     = findByName n xs

-- Total score of ranked items (uses only Scored)
totalScore :: Scored a => [a] -> Integer
totalScore []     = 0
totalScore (x:xs) = score x + totalScore xs

main :: IO ()
main = do
    let alice = Player "Alice" 95
    let bob   = Player "Bob" 80
    let carol = Player "Carol" 95

    -- Named typeclass
    assert (getName alice == "Alice") "getName player"
    assert (findByName "Bob" [alice, bob, carol] == True) "find bob"
    assert (findByName "Dave" [alice, bob, carol] == False) "not found"

    -- Scored typeclass
    assert (getScore alice == 95) "getScore alice"
    assert (higherScore alice bob == True) "alice higher"
    assert (higherScore bob carol == False) "bob not higher"
    assert (totalScore [alice, bob, carol] == 270) "total score"

    -- Ranked typeclass (internally uses Named + Scored via instance methods)
    assert (getRank alice == "Alice:95") "rank alice"
    assert (getRank bob == "Bob:80") "rank bob"

    -- Same typeclasses satisfied for Team
    let t = Team "Rockets" 100
    let t2 = Team "Lakers" 85
    assert (getName t == "Rockets") "team name"
    assert (getScore t == 100) "team score"
    assert (getRank t == "Rockets:100") "team rank"
    assert (higherScore t t2 == True) "rockets higher"
    assert (totalScore [t, t2] == 185) "team total"
    assert (findByName "Lakers" [t, t2] == True) "find lakers"

    putStrLn "ok"
