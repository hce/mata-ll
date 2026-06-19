-- METAR parser using parser combinators
-- Parses aviation routine weather reports (ICAO format)

import LString

-- Parser combinator library (monadic, position-based on a string)
-- A Parser a is: String -> Integer -> Maybe (a, Integer)
-- We represent this as a newtype for clarity, but since MLL newtypes
-- are zero-cost, this has no overhead.

newtype Parser a = Parser (String -> Integer -> Maybe (a, Integer))

runParser :: Parser a -> String -> Maybe a
runParser (Parser p) s = case p s 1 of
    Just (result, _) -> Just result
    Nothing          -> Nothing

-- Monad instance for Parser
instance Functor Parser where
    fmap f (Parser p) = Parser (\s i -> case p s i of
        Just (a, j) -> Just (f a, j)
        Nothing     -> Nothing)

instance Applicative Parser where
    pure x = Parser (\_ i -> Just (x, i))
    (<*>) (Parser pf) (Parser pa) = Parser (\s i -> case pf s i of
        Just (f, j) -> case pa s j of
            Just (a, k) -> Just (f a, k)
            Nothing     -> Nothing
        Nothing -> Nothing)

instance Monad Parser where
    (>>=) (Parser p) f = Parser (\s i -> case p s i of
        Just (a, j) -> case f a of
            Parser q -> q s j
        Nothing     -> Nothing)

-- Combinators

-- Fail unconditionally
pfail :: Parser a
pfail = Parser (\_ _ -> Nothing)

-- Try first, fall back to second
(<|>) :: Parser a -> Parser a -> Parser a
(<|>) (Parser p) (Parser q) = Parser (\s i -> case p s i of
    Just result -> Just result
    Nothing     -> q s i)

infixl 3 <|>

-- Consume one character satisfying a predicate
satisfy :: (Integer -> Bool) -> Parser String
satisfy pred = Parser (\s i ->
    if i > strLen s then Nothing
    else let c = strByte s i
         in if pred c then Just (strChar c, i + 1)
            else Nothing)

-- Consume a specific character (by ASCII code)
char :: Integer -> Parser String
char expected = satisfy (\c -> c == expected)

-- Consume a specific string literal
string :: String -> Parser String
string lit = Parser (\s i ->
    let len = strLen lit
        end = i + len - 1
    in if end > strLen s then Nothing
       else if strSub s i end == lit then Just (lit, i + len)
            else Nothing)

-- Zero or more
many :: Parser a -> Parser [a]
many p = manyAcc p []

manyAcc :: Parser a -> [a] -> Parser [a]
manyAcc p acc = (do
    x <- p
    manyAcc p (x : acc)) <|> pure (reverse acc)

-- One or more
many1 :: Parser a -> Parser [a]
many1 p = do
    x <- p
    xs <- many p
    pure (x : xs)

-- Optional
optional :: Parser a -> Parser (Maybe a)
optional p = (fmap Just p) <|> pure Nothing

-- Digit (ASCII 48-57)
digit :: Parser String
digit = satisfy (\c -> c >= 48 && c <= 57)

-- Letter (uppercase ASCII 65-90)
upper :: Parser String
upper = satisfy (\c -> c >= 65 && c <= 90)

-- Any non-space character
nonSpace :: Parser String
nonSpace = satisfy (\c -> c /= 32)

-- Parse a natural number (sequence of digits)
number :: Parser Integer
number = do
    ds <- many1 digit
    pure (read (strConcat ds))

-- Parse exactly N digits as a number
digits :: Integer -> Parser Integer
digits n = do
    ds <- count n digit
    pure (read (strConcat ds))

-- Parse exactly N items
count :: Integer -> Parser a -> Parser [a]
count 0 _ = pure []
count n p = do
    x <- p
    xs <- count (n - 1) p
    pure (x : xs)

-- Skip a space
space :: Parser ()
space = do
    _ <- char 32
    pure ()

-- Concatenate a list of single-char strings
strConcat :: [String] -> String
strConcat [] = ""
strConcat (x:xs) = x <> strConcat xs

-- End of input
eof :: Parser ()
eof = Parser (\s i -> if i > strLen s then Just ((), i) else Nothing)

-- Peek without consuming
peek :: Parser Integer
peek = Parser (\s i ->
    if i > strLen s then Nothing
    else Just (strByte s i, i))

-- Skip until space or end
skipToken :: Parser String
skipToken = do
    cs <- many1 nonSpace
    pure (strConcat cs)

------------------------------------------------------------------------
-- METAR data types
------------------------------------------------------------------------

data WindDir = WindDeg Integer | WindVariable
    deriving Show

data Wind = Wind
    { windDir   :: WindDir
    , windSpeed :: Integer
    , windGust  :: Maybe Integer
    , windUnit  :: String }
    deriving Show

data Visibility = VisMiles Integer
                | VisMetres Integer
                | VisCavok
    deriving Show

data CloudAmount = Few | Scattered | Broken | Overcast
    deriving Show

data CloudLayer = CloudLayer CloudAmount Integer
                | VerticalVisibility Integer
                | SkyClear
    deriving Show

data Metar = Metar
    { metarStation    :: String
    , metarDay        :: Integer
    , metarHour       :: Integer
    , metarMinute     :: Integer
    , metarWind       :: Wind
    , metarVisibility :: Visibility
    , metarClouds     :: [CloudLayer]
    , metarTempC      :: Integer
    , metarDewpointC  :: Integer
    , metarAltimeter  :: Maybe Integer }
    deriving Show

------------------------------------------------------------------------
-- METAR field parsers
------------------------------------------------------------------------

-- Optional "METAR" prefix
metarPrefix :: Parser ()
metarPrefix = (do
    _ <- string "METAR"
    space
    pure ()) <|> pure ()

-- Station identifier: 4 uppercase letters
station :: Parser String
station = do
    cs <- count 4 upper
    pure (strConcat cs)

-- Day/time group: DDHHMMz
timeGroup :: Parser (Integer, Integer, Integer)
timeGroup = do
    day  <- digits 2
    hour <- digits 2
    min  <- digits 2
    _ <- char 90  -- 'Z'
    pure (day, hour, min)

-- Wind: dddssKT or dddssGggKT or VRBssKT
wind :: Parser Wind
wind = variableWind <|> normalWind

variableWind :: Parser Wind
variableWind = do
    _ <- string "VRB"
    spd <- digits 2 <|> digits 3
    gust <- optionalGust
    unit <- parseWindUnit
    pure (Wind { windDir = WindVariable
               , windSpeed = spd
               , windGust = gust
               , windUnit = unit })

normalWind :: Parser Wind
normalWind = do
    dir <- digits 3
    spd <- digits 2 <|> digits 3
    gust <- optionalGust
    unit <- parseWindUnit
    pure (Wind { windDir = WindDeg dir
               , windSpeed = spd
               , windGust = gust
               , windUnit = unit })

optionalGust :: Parser (Maybe Integer)
optionalGust = (do
    _ <- char 71  -- 'G'
    g <- digits 2 <|> digits 3
    pure (Just g)) <|> pure Nothing

parseWindUnit :: Parser String
parseWindUnit = string "KT" <|> string "MPS" <|> string "KMH"

-- Visibility
visibility :: Parser Visibility
visibility = cavok <|> visSM <|> visMetres

cavok :: Parser Visibility
cavok = do
    _ <- string "CAVOK"
    pure VisCavok

visSM :: Parser Visibility
visSM = do
    n <- number
    _ <- string "SM"
    pure (VisMiles n)

visMetres :: Parser Visibility
visMetres = do
    n <- number
    pure (VisMetres n)

-- Cloud layers
cloudLayer :: Parser CloudLayer
cloudLayer = vv <|> skyClear <|> cloudWithHeight

skyClear :: Parser CloudLayer
skyClear = do
    _ <- string "SKC" <|> string "CLR" <|> string "NCD" <|> string "NSC"
    pure SkyClear

vv :: Parser CloudLayer
vv = do
    _ <- string "VV"
    h <- digits 3
    pure (VerticalVisibility (h * 100))

cloudWithHeight :: Parser CloudLayer
cloudWithHeight = do
    amt <- cloudAmount
    h   <- digits 3
    -- skip optional CB/TCU suffix
    _ <- optional (string "CB" <|> string "TCU")
    pure (CloudLayer amt (h * 100))

cloudAmount :: Parser CloudAmount
cloudAmount = (string "FEW" >>= \_ -> pure Few) <|> (string "SCT" >>= \_ -> pure Scattered) <|> (string "BKN" >>= \_ -> pure Broken) <|> (string "OVC" >>= \_ -> pure Overcast)

-- Temperature / dewpoint: (M)?dd/(M)?dd
temperature :: Parser (Integer, Integer)
temperature = do
    t <- signedTemp
    _ <- char 47  -- '/'
    d <- signedTemp
    pure (t, d)

signedTemp :: Parser Integer
signedTemp = negTemp <|> posTemp

negTemp :: Parser Integer
negTemp = do
    _ <- char 77  -- 'M'
    n <- digits 2
    pure (0 - n)

posTemp :: Parser Integer
posTemp = digits 2

-- Altimeter: A followed by 4 digits (hundredths of inHg)
altimeter :: Parser Integer
altimeter = do
    _ <- char 65  -- 'A'
    n <- digits 4
    pure n

-- Parse cloud layers (zero or more, separated by spaces)
clouds :: Parser [CloudLayer]
clouds = many (do
    cl <- cloudLayer
    _ <- optional space
    pure cl)

-- Skip a token we don't care about (weather phenomena, etc.)
skipUnknown :: Parser ()
skipUnknown = do
    _ <- skipToken
    pure ()

------------------------------------------------------------------------
-- Full METAR parser
------------------------------------------------------------------------

metar :: Parser Metar
metar = do
    metarPrefix
    stn <- station
    space
    (day, hour, min) <- timeGroup
    space
    w <- wind
    space
    vis <- visibility
    space
    cls <- clouds
    (temp, dew) <- temperature
    alt <- optional (do
        space
        altimeter)
    -- ignore remarks
    pure (Metar { metarStation    = stn
                , metarDay        = day
                , metarHour       = hour
                , metarMinute     = min
                , metarWind       = w
                , metarVisibility = vis
                , metarClouds     = cls
                , metarTempC      = temp
                , metarDewpointC  = dew
                , metarAltimeter  = alt })

------------------------------------------------------------------------
-- Pretty-printing
------------------------------------------------------------------------

showWindDir :: WindDir -> String
showWindDir WindVariable = "variable"
showWindDir (WindDeg d) = show d <> "deg"

showVis :: Visibility -> String
showVis VisCavok = "CAVOK"
showVis (VisMiles n) = show n <> "SM"
showVis (VisMetres n) = show n <> "m"

showCloudAmt :: CloudAmount -> String
showCloudAmt Few = "FEW"
showCloudAmt Scattered = "SCT"
showCloudAmt Broken = "BKN"
showCloudAmt Overcast = "OVC"

showCloud :: CloudLayer -> String
showCloud SkyClear = "clear"
showCloud (VerticalVisibility h) = "VV " <> show h <> "ft"
showCloud (CloudLayer amt h) = showCloudAmt amt <> " " <> show h <> "ft"

joinWith :: String -> [String] -> String
joinWith _ []     = ""
joinWith _ [x]    = x
joinWith sep (x:xs) = x <> sep <> joinWith sep xs

showClouds :: [CloudLayer] -> String
showClouds [] = "clear"
showClouds cs = joinWith ", " (map showCloud cs)

showMetar :: Metar -> String
showMetar m =
    "Station:     " <> metarStation m <> "\n"
    <> "Time:        day " <> show (metarDay m) <> " " <> show (metarHour m) <> ":" <> show (metarMinute m) <> "Z\n"
    <> "Wind:        " <> showWindDir (windDir (metarWind m)) <> " at " <> show (windSpeed (metarWind m)) <> windUnit (metarWind m)
    <> (case windGust (metarWind m) of
            Nothing -> ""
            Just g  -> " gusting " <> show g) <> "\n"
    <> "Visibility:  " <> showVis (metarVisibility m) <> "\n"
    <> "Clouds:      " <> showClouds (metarClouds m) <> "\n"
    <> "Temperature: " <> show (metarTempC m) <> "C / dewpoint " <> show (metarDewpointC m) <> "C\n"
    <> (case metarAltimeter m of
            Nothing -> ""
            Just a  -> "Altimeter:   " <> show a <> " (x0.01 inHg)\n")

------------------------------------------------------------------------
-- Tests
------------------------------------------------------------------------

main :: IO ()
main = do
    -- Test 1: standard US METAR
    let input1 = "METAR KJFK 121856Z 31009KT 10SM FEW250 M04/M19 A3049"
    case runParser metar input1 of
        Nothing -> putStrLn "FAIL: could not parse input1"
        Just m  -> do
            assert (metarStation m == "KJFK") "station"
            assert (metarDay m == 12) "day"
            assert (metarHour m == 18) "hour"
            assert (metarMinute m == 56) "minute"
            assert (windSpeed (metarWind m) == 9) "wind speed"
            assert (metarTempC m == -4) "temp"
            assert (metarDewpointC m == -19) "dewpoint"
            assert (metarAltimeter m == Just 3049) "altimeter"
            putStrLn "Test 1 (KJFK): OK"
            putStrLn (showMetar m)

    -- Test 2: CAVOK, variable wind
    let input2 = "EDDM 051020Z VRB03KT CAVOK 22/14 A2998"
    case runParser metar input2 of
        Nothing -> putStrLn "FAIL: could not parse input2"
        Just m  -> do
            assert (metarStation m == "EDDM") "station2"
            assert (metarDay m == 5) "day2"
            case windDir (metarWind m) of
                WindVariable -> putStr "."
                _            -> putStrLn "FAIL: expected variable wind"
            assert (metarTempC m == 22) "temp2"
            assert (metarDewpointC m == 14) "dewpoint2"
            putStrLn "Test 2 (EDDM): OK"
            putStrLn (showMetar m)

    -- Test 3: gusts, multiple cloud layers
    let input3 = "EGLL 201250Z 24015G27KT 9999 SCT040 BKN100 17/09 A2984"
    case runParser metar input3 of
        Nothing -> putStrLn "FAIL: could not parse input3"
        Just m  -> do
            assert (metarStation m == "EGLL") "station3"
            assert (windGust (metarWind m) == Just 27) "gust3"
            assert (windSpeed (metarWind m) == 15) "speed3"
            assert (length (metarClouds m) == 2) "clouds3"
            assert (metarTempC m == 17) "temp3"
            putStrLn "Test 3 (EGLL): OK"
            putStrLn (showMetar m)

    putStrLn "All METAR tests passed!"
