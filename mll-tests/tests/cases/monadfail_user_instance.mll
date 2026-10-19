newtype Parser a = Parser (String -> Maybe (a, String))

runParser :: Parser a -> String -> Maybe (a, String)
runParser (Parser f) s = f s

instance Functor Parser where
    fmap f (Parser p) = Parser (\s -> case p s of
        Nothing -> Nothing
        Just (a, rest) -> Just (f a, rest))

instance Applicative Parser where
    pure x = Parser (\s -> Just (x, s))
    liftA2 f (Parser pa) (Parser pb) = Parser (\s -> case pa s of
        Nothing -> Nothing
        Just (a, r1) -> case pb r1 of
            Nothing -> Nothing
            Just (b, r2) -> Just (f a b, r2))

instance Monad Parser where
    (>>=) (Parser p) k = Parser (\s -> case p s of
        Nothing -> Nothing
        Just (a, rest) -> runParser (k a) rest)

instance MonadFail Parser where
    fail _ = Parser (\_ -> Nothing)

item :: Parser (Maybe Int)
item = Parser (\s -> if s == "" then Just (Nothing, s) else Just (Just 7, s))

needJust :: Parser Int
needJust = do
    Just x <- item
    return (x * 2)

pairOf :: Parser (Int, Int)
pairOf = do
    a <- needJust
    (b, c) <- return (a, a + 1)
    return (b, c)

giveUp :: Parser Int
giveUp = fail "no"

main :: IO ()
main = do
    print (fmap fst (runParser needJust "abc"))
    print (fmap fst (runParser needJust ""))
    print (fmap fst (runParser pairOf "abc"))
    print (fmap fst (runParser pairOf ""))
    print (fmap fst (runParser giveUp "abc"))
