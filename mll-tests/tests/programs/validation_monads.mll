-- Validation: parsing user records through Either with do-notation,
-- Maybe chains, mapM/traverse/sequence over both, and an
-- error-accumulating Validation applicative that is deliberately not a
-- monad (errors accumulate in a String, the Semigroup mata-ll shares with
-- GHC; `<>` at list types is a documented mata-ll deviation).
-- Leans on: the Either and Maybe monads, Traversable-generic mapM and
-- sequence, a user Functor/Applicative instance with liftA2, Data.Maybe
-- helpers, Show of nested Either/Maybe/tuple values, Semigroup on String.
import LString
import Data.Maybe (catMaybes, mapMaybe, fromMaybe, isJust)

data User = User
    { userName :: String
    , userAge :: Int
    , userEmail :: String
    }
    deriving (Show, Eq)

isDigit :: Int -> Bool
isDigit c = c >= 48 && c <= 57

parseInt :: String -> Maybe Int
parseInt s = case strToInts s of
    [] -> Nothing
    cs | all isDigit cs -> Just (foldl (\acc d -> acc * 10 + (d - 48)) 0 cs)
    _ -> Nothing

validateName :: String -> Either String String
validateName n
    | strLen n == 0 = Left "name is empty"
    | strLen n > 12 = Left ("name too long: " <> n)
    | otherwise = Right n

validateAge :: String -> Either String Int
validateAge s = case parseInt s of
    Nothing -> Left ("age is not a number: " <> s)
    Just a
        | a < 0 || a > 150 -> Left ("age out of range: " <> show a)
        | otherwise -> Right a

validateEmail :: String -> Either String String
validateEmail e
    | elem 64 codes && elem 46 (drop 1 (dropWhile (/= 64) codes)) = Right e
    | otherwise = Left ("bad email: " <> e)
  where codes = strToInts e

mkUser :: (String, String, String) -> Either String User
mkUser (n, a, e) = do
    name <- validateName n
    age <- validateAge a
    email <- validateEmail e
    Right (User { userName = name, userAge = age, userEmail = email })

-- Accumulating validation: every failure is collected.
data Validation e a = Failure e | Success a
    deriving Show

instance Functor (Validation e) where
    fmap _ (Failure e) = Failure e
    fmap f (Success a) = Success (f a)

instance Semigroup e => Applicative (Validation e) where
    pure = Success
    (<*>) (Failure e1) (Failure e2) = Failure (e1 <> e2)
    (<*>) (Failure e1) (Success _) = Failure e1
    (<*>) (Success _) (Failure e2) = Failure e2
    (<*>) (Success f) (Success a) = Success (f a)
    liftA2 f (Failure e1) (Failure e2) = Failure (e1 <> e2)
    liftA2 _ (Failure e1) (Success _) = Failure e1
    liftA2 _ (Success _) (Failure e2) = Failure e2
    liftA2 f (Success a) (Success b) = Success (f a b)

liftE :: Either String a -> Validation String a
liftE (Left e) = Failure (e <> "; ")
liftE (Right a) = Success a

mkUserAll :: (String, String, String) -> Validation String User
mkUserAll (n, a, e) =
    liftA2 (\(name, age) email -> User { userName = name, userAge = age, userEmail = email })
        (liftA2 (\name age -> (name, age)) (liftE (validateName n)) (liftE (validateAge a)))
        (liftE (validateEmail e))

inputs :: [(String, String, String)]
inputs =
    [ ("alice", "30", "alice@example.com")
    , ("", "30", "alice@example.com")
    , ("bob", "abc", "bob@example.com")
    , ("carol", "200", "carol@example")
    , ("a-very-long-name-indeed", "-1", "nope")
    , ("dave", "0", "dave@x.y")
    ]

safeDiv :: Int -> Int -> Maybe Int
safeDiv _ 0 = Nothing
safeDiv a b = Just (a `div` b)

chain :: Int -> Int -> Int -> Maybe Int
chain a b c = do
    x <- safeDiv a b
    y <- safeDiv x c
    let z = x + y
    if z > 100 then Nothing else Just z

main :: IO ()
main = do
    mapM_ (print . mkUser) inputs
    print (mapM mkUser (take 1 inputs))
    print (mapM mkUser inputs)
    print (fmap (map userName) (mapM mkUser [inputs !! 0, inputs !! 5]))
    print (sequence [Right 1, Right 2, Left "boom", Left "later" :: Either String Int])
    print (sequence [Just 1, Just 2, Just 3], sequence [Just 1, Nothing, Just 3])
    print (traverse (\x -> if x > 0 then Just x else Nothing) [3, 2, 1], traverse (\x -> if x > 0 then Just x else Nothing) [3, 0, 1])
    print (mapM_ (\x -> if x > 2 then Left x else Right ()) [1, 2, 3, 4], mapM_ (\x -> if x > 9 then Left x else Right ()) [1, 2, 3, 4])
    mapM_ (print . mkUserAll) inputs
    print (chain 100 5 2, chain 100 0 2, chain 100 5 0, chain 1000 1 1)
    print (catMaybes [chain 100 5 2, chain 100 0 2, Just 7], mapMaybe (\b -> safeDiv 12 b) [1, 0, 2, 0, 3])
    print (fromMaybe (-1) (chain 1 0 1), isJust (chain 9 3 1))
    print (fmap userAge (mkUser (inputs !! 0)), fmap userAge (mkUser (inputs !! 1)))
    print (either' (mkUser (inputs !! 2)), either' (mkUser (inputs !! 0)))
    print (fmap (+ 1) (Success 1 :: Validation String Int), fmap (+ 1) (Failure "no" :: Validation String Int))
    print (liftA2 (+) (Failure "a") (Failure "b") :: Validation String Int)
  where
    either' (Left e) = "failed: " <> e
    either' (Right u) = "ok: " <> userName u
