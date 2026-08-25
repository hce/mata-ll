-- The newtype record form with the brace on the NEXT line.  Regression:
-- the braced-constructor detection tested the immediately following
-- token for `{`, so an intervening layout token broke the record form
-- ("Expected …") while the equivalent data declaration accepted it.
-- (Record-syntax CONSTRUCTION of a newtype — `MkAge { unAge = n }` — is
-- a separate pre-existing gap, recorded in the round-3 findings.)

newtype Age = MkAge
    { unAge :: Int }
    deriving (Show)

newtype Plain = Plain (Maybe Int)

unwrapOr :: Plain -> Int -> Int
unwrapOr p d = case p of
    Plain (Just n) -> n
    Plain Nothing -> d

main :: IO ()
main = do
    print (unAge (MkAge 41))
    print (MkAge 7)
    print (unwrapOr (Plain (Just 3)) 0)
    print (unwrapOr (Plain Nothing) 9)
