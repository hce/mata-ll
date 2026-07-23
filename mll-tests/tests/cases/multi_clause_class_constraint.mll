-- Regression: a class method used in a LATER clause of a multi-clause
-- function. Each clause is checked against the same signature, and composing
-- the per-clause substitutions used to drop the second clause's binding of
-- the shared signature variable ("first clause wins"), severing its body
-- types from the signature. The class-method use inside clause 2 then could
-- not be related to the concrete instantiation; an accidental "default every
-- unmapped variable to the single type parameter" rule in the monomorphizer
-- papered over it (and mis-resolved other programs). The substitutions are
-- now merged by unification, and the defaulting rule is gone.

class Named a where
    getName :: a -> String

data Player = Player String Int deriving (Show, Eq)
data Team = Team String deriving (Show, Eq)

instance Named Player where
    getName (Player n _) = n

instance Named Team where
    getName (Team n) = n

-- The method use sits in clause 2 (and its guard), not clause 1.
findByName :: Named a => String -> [a] -> Bool
findByName _ []     = False
findByName n (x:xs)
    | getName x == n = True
    | otherwise      = findByName n xs

-- Same shape with the method in a where-binding of a later clause.
describeAll :: Named a => [a] -> String
describeAll []     = ""
describeAll (x:xs) = entry <> describeAll xs
    where entry = getName x <> ";"

main :: IO ()
main = do
    let ps = [Player "Alice" 95, Player "Bob" 80]
    assert (findByName "Bob" ps == True) "find bob"
    assert (findByName "Eve" ps == False) "eve missing"
    assert (describeAll ps == "Alice;Bob;") "describe players"
    let ts = [Team "Rockets", Team "Lakers"]
    assert (findByName "Lakers" ts == True) "find lakers"
    assert (describeAll ts == "Rockets;Lakers;") "describe teams"
    putStrLn "ok"
